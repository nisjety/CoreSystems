# Grok Bot 0.70.2 - agent-product deep dive (Audit 1, facet "agent-product")

Auditor: deep-dive analyst, Sonnet. Read-only audit. Nothing was launched, installed, built or modified. No per-user data stores, cookies or tokens were opened. Companion file: `grokbot.agent-product.features.json` (171 records, ids `grokbot-agent-product-001` to `-171`; below they are cited as `#001` to `#171`). A first pass produced #001 to #138; a resume pass after a usage-limit interruption verified a sample of those records against the extracted source (no defects found) and appended #139 to #171 (section 3.12).

Provenance tags used throughout: **REA** = observed through REA analysis of the shipped artifact; **SRC** = read directly in the shipped bundles, protobuf descriptors or the Lingui message catalogs of the extracted copy; **DOC** = documentation (none ships with the app; used only for the Verevon baseline files); **INF** = my inference, basis stated. Confidence is `high` unless stated.

## 0. Method, REA usage and what this audit can and cannot say

**Evidence base.** The extracted copy of `app.asar` (`extract/grokbot/dist`, 672 files). The renderer is minified with no source maps, so I read it three ways: (1) the compiled **English Lingui catalogs** (about 3,700 messages: core 1,606; chat 971; settings 531; agents 527; shared 241; the scripted Team Bot dialogue 99), which give the exact UI vocabulary and, through message ids, a way to tie every UI claim to one catalog entry; (2) the **protobuf descriptors** in `proto.cjs` (the app embeds all `aiserver.v1` message schemas as compact strings such as `GrokBotAgentLiveState|1 agent_id 9|2 is_running 8|...`, plus 269 enums), which are the most reliable statement of the data model; (3) targeted byte-offset reads of the coordinator (`node-agent-coordinator/main.cjs`), the local-exec daemon, `main-app.cjs`, `main-core.cjs` and the preloads.

**Evidence pointer format.** Every record cites `path@byte-offset` (offset into the minified file, resolvable with `indexOf`), `schema <Message>` / `enum <Name>` / `rpc <Method>` (offset of the descriptor in `dist/electron-main/proto.cjs`), or `lingui-id <id>` (the id in the English catalog inside `dist/renderer/assets/index.eager-app-CY939ORZ.js`). The generator that produced the file resolved every pointer against the extracted copy at build time and refused to write on any miss, so no record cites something that is not there. All paths are relative to `.../scratchpad/extract/grokbot/`.

**REA tools actually used** (honest log):

| REA tool | Use | Result |
|---|---|---|
| `analyze_javascript_application` | First pass: **not re-run**, the scout's whole-asar analysis (`C:/Program Files/Grok Bot/resources/app.asar`, record-and-continue) was reused. Resume pass: that evidence was no longer retained in the session, so I ran **one targeted analysis** on the extracted `dist/electron-main` directory (format directory, record-and-continue, 2048 MB heap, summary). | Resume-pass result (parent `ev_8bbbeb6af2c1945c665aa2189e9cae6fb8af29f6b0530dde33217e5dda4397b2`, view `ev_f3aeb71feb0d39ac06e6725f386c5d4c45cd54b1e0471c52892d27d96d9c81e4`): 41 JS files parsed, 1,172,142 AST nodes, 830 findings, 4 browser windows, 11 web-preference sets, 6 main handlers, 2 utility processes; graph and semantic coverage partial because `main-app.cjs` hit a call-stack limit, `main-core.cjs` exhausted the 2 GB worker heap and the 1Password service exceeded the result budget. Scout result for the whole asar, parent evidence `ev_2e557cc3fca76ed3a60cb306d239aeb8ceb38de4a443da2325746bda4cd61298`: 616 relevant files, 600 parsed JS, 5,217 findings, 4 browser windows, 11 web-preference sets, 6 context-bridge APIs, 15 IPC operations (13 dynamic), 2 utility processes, 5 integrity contradictions on native files. |
| `inspect_analysis_view` | Module page (offset 0, 5 items of 766) and module item for `dist/node-agent-coordinator/main.cjs` (node `jag_node_b88d4b6b...`). | Coordinator: 207,397 AST nodes, 31 `require` references, 6 network endpoints, zero storage or bundler registrations. Module views carry structure only, not strings, so they could not substitute for source reading. |
| `trace_application_feature` | Resume pass: a retained-evidence reference to the scout's old evidence was rejected (not retained in this session, an integrity error, so nothing was inferred from it); then two seeds on the new retained evidence, api `setBadgeCount` and string `link-preview-request`, both `no-match` because the semantic graph of `main-app.cjs` failed (call-stack limit above), so REA can neither confirm nor deny those features; they were confirmed by reading the source (#155 to #157, #144, #145). First pass: two seeds: string `CreateGrokBotRoom` (ev `ev_159436e7...`) and api `setAsDefaultProtocolClient` (ev `ev_ad958229...`). | Both `no-match` with a partial source graph, as the scout's four seeds also were. Not evidence of absence. Both features were then confirmed in source. |
| `inspect_pe_resources` | Not re-run (scout result reused: ELECTRONASAR integrity resource in `Grok Bot.exe`). | Cited in #131 and #138 via the parent evidence. |

REA did not tell me anything the source did not; its value here is the process and window census and the integrity note. **Not used:** `list_strings`, `open_binary`, `binary_session` (shared active target, not needed). `search_strings` and `inspect_artifact` were loaded in the resume pass but not needed (they target native binaries and archives; the program files are Electron JavaScript already extracted). Net value of REA in this audit remains the process, window and handler census plus an honest record of where static graph recovery fails on large minified bundles; every feature record is grounded in direct source reading.

**Hard limits.** (1) Static only: the app was never run, so no runtime behaviour, server logic, quotas, plan gating or feature-flag state was observed. Server behaviour is inferred from descriptors, enums and UI copy. (2) This is the **Windows** install: several features are explicitly macOS or Linux only (see section 6) and some server-side RPCs may not be reachable from Windows. (3) Marketing-style copy (template descriptions) proves intent, not behaviour. (4) Absence claims ("no human DMs") are grep-level and medium confidence.

## 1. What Grok Bot is, in one page

Grok Bot (package `sand`, built by SpaceXAI on the Cursor platform: `aiserver.v1.*` services, `@anysphere/*` packages, Cursor hosts) is a desktop and mobile client for **personal and team AI agents called Bots** [SRC, high]. The product premise, in its own words, is that a Bot "has its own computer and works just like you" (#113). The five ideas that matter for Verevon:

1. **A Bot is a durable named identity, not a chat thread.** It has role, avatar, voice, owner, visibility, a harness (where it runs) and many *sessions* (main chat, teammate DMs, group chats, Slack and Teams threads). (#001, #003, #057)
2. **Each account gets a persistent cloud computer ("Box")** that Bots drive (shell, files, browser, desktop), that hibernates when idle, can be watched live over VNC, taken over by the human, snapshotted, updated and recovered. The user's own PC is a second, optional workplace reached through a server-queued reverse channel. (#015 to #023)
3. **Autonomy is governed by an approval ladder**, not by a mode switch: always allow / needs approval / blocked per tool, inline cards with allow once / always / deny / never, natural-language Auto-review rules layered user over team over built-in, per-person addressing in shared contexts, and hardware-backed approvals for purchases. (#026 to #036)
4. **Bots are teammates**: publish a Bot to the team, teammates get their own sessions and per-person connector accounts, the Bot appears in Slack and Teams as its own app, wakes on events, runs routines, keeps private and team memory. (#003, #004, #039 to #045, #078, #079)
5. **The interface narrates agent work** (live state, task states, activity phrases, needs-you queue) and is built around the human supervising rather than the human chatting. (#012, #013, #076, #112, #114)

What it is **not**: a human collaboration product. There are no human-to-human DMs, channels, edits, deletes, reactions between humans, huddles or search over messages; humans talk to Bots, to each other only inside group chats whose members are Bots (plus people added as participants), and through Slack and Teams, which Grok Bot integrates with as an app (section 6). [SRC absence, medium; INF: collaboration between humans is delegated to Slack and Teams.]

## 2. Architecture

### 2.1 Processes and surfaces [REA + SRC]

- **Electron 42.1.0 / Chromium 148** shell with a main process, 4 browser windows (REA), sandboxed React 19 renderer (Base UI, StyleX, TanStack Query, Tiptap 3, Lingui 5). The renderer sees four main-world objects: `desktop`, `coordinatorPort`, `sandRenderTrace`, `desktopMount` (#106).
- **`node-agent-coordinator`** (utility process, 882 KB): owns the gateway connection, the **send journal**, transcript **replicas**, the roster, WebAuthn and cookie-approval providers and the voice-call harness. Renderers attach by receiving a **MessagePort** (`sand:coordinator-port`) and speak a versioned `hello`/`ready`/`request`/`reply`/`event` protocol; protocol breaches settle the session (#089).
- **`local-exec-daemon`** (3.5 MB, detached): shell with sandbox policy and bash AST classification, MCP, file tools, computer-use client for the macOS helper, Messages helper, a reverse local-exec channel client (#023, #024, #035).
- Native: `sand-webauthn-signer.exe`, notification-settings addon, process-list addon, tree-sitter(-bash); `onepassword-connection-service.cjs`; `chrome-import-worker.cjs` (node:sqlite).
- Floating **desktop mount panel** (always-on-top transparent 320x176 window) on macOS/Linux only (#107).

### 2.2 Protocols [SRC]

- **Connect RPC over HTTP/2** with protobuf-es v1: 7 services, 484 methods (GrokBotService 309, DashboardService 73, SandBoxService 55, AgentStoreService 24, AiService 8, AnalyticsService 8, BackgroundComposerService 7). Server streaming for `WatchGrokBotTranscripts`, `WatchGrokBotUserComputerRequests`, `WatchSandBoxMigration`; unary elsewhere; a polling fallback exists for the desktop reverse channel (#088, #092).
- **WebSocket** for the realtime voice session and noVNC/RFB for the Box desktop.
- **W3C `traceparent`** and client timestamps on every send (#091).
- Auth: OAuth 2.0 code with PKCE, refresh tokens, per-account slots; passkeys/WebAuthn; MCP OAuth and Slack/Teams/GitHub/1Password connect flows via browser hand-off and loopback relays (#067, #109).

### 2.3 Data model (condensed) [SRC, protobuf]

```
Account/User --1..n--> GrokBotAgent{id, name, role, avatar, voice, harness BOX|TEMPORAL,
                         kind AGENT|ROOM, visibility OWNER|TEAM, team_id, published_at,
                         member_agent_ids (rooms), people (rooms), drafting_disabled}
GrokBotAgent --1..n--> GrokBotAgentSession{session_id, kind MAIN|DM|GROUP|SLACK_DM|SLACK_THREAD|TEAMS_*}
Session --> transcript: GrokBotTranscriptEntry{seq, updated_seq, entry_kind, body|blob_hash}
                         keyed (agent_id, generation, session_id); generation bumps on fresh/rollback
Agent --> LiveState{is_running, composing, retrying, activity{kind,tool,detail,target,call_id},
                    awaiting{tab_id,reason,since}, stale_after_ms, box_handoff_*, active_group_member_id}
Agent x User --> ClientState{last_viewed, unread_count, notifications_enabled, notify_on_updates_enabled,
                    hidden_from_sidebar, last_entry_preview}
Agent --> Tasks{state WORKING|NEEDS_INPUT|DONE}, Subagents, Automations(routines), Skills, Plugins,
          Secrets(write-only), Files, MemoryShards{scope, scope_key, profile+logs}, Email inbox,
          Slack/Teams connections, AccessState{principals[user|group, role], team_row_on, state_token}
Agent --> Box (per account/session): RunState ABSENT|HIBERNATED|RUNNING|STARTING, disk pressure,
          upgrade schedule, store manifest snapshots (COMMITTED|RETRY|FENCED)
Desktop --> Machine{label, kind, capabilities{shell, messages_op, local_standing_grants, mobile_local_tools}}
```

Sources: #001, #003, #012, #056, #057, #088, #136. A Room is itself an agent of kind ROOM, with `member_agent_ids` and `people`. The same identity can run on two runtimes (**BOX** per-Bot computer or **TEMPORAL** durable server harness) with a rollout state machine OFF/SHADOW/LIVE/BOX and migration passes (#092) [SRC; the Temporal.io reading is INF, medium].

### 2.4 One send, end to end (INF where marked)

1. Composer (Tiptap) builds text plus `rich_text`; the client **journals** the send (nonce plus accepted digest), shows it as pending (#056).
2. Coordinator forwards `SendGrokBotUserMessage{message_id, session_id, reply_to_id, task_id, machine_id, traceparent, mcp_config_json, initiator, send_source}`; the response says `ACCEPTED_BOX | ACCEPTED_TEMPORAL | DUPLICATE | REFUSED` with an optional workflow id. Unknown acceptance is a first-class status `UNKNOWN_DURABILITY` (#056).
3. Rows arrive on the multiplexed watch stream; the pending bubble confirms when the **echo** lands (state `accepted-awaiting-echo`); offline sends sit as `queued_offline` and flush with nonce and digest checks (SAND-E0703 to E0707) (#056, #088).
4. While the Bot works, `LiveState` frames drive the sidebar and header (activity, awaiting, retrying), and tool activity becomes friendly phrases (#012, #114).
5. If the Bot needs a decision it emits an approval card or a widget; the user's resolution is an RPC (`ResolveGrokBot...`, `RespondGrokBotWidget`), and the Bot **resumes**; resume failures have named codes ("failed to resume after reaction / box handoff / MCP auth / host update / form submission") (#027, #093, #130).
6. [INF] The Box-side runtime executes tools; local-machine tools are queued to the desktop as request frames with authorization provenance (#023, #100).

## 3. Feature deep dives (facet focus: categories 7, 8, 11, 12; 1 and 5 where present)

### 3.1 Creating and configuring agents (7, 12)

- **Job-first creation**: the new-agent pane asks to "name a teammate and describe what they should do"; onboarding collects job role and tools and recommends from **74 ready-made Bots** whose eligibility is universal, job-role (9 roles) or selected-tools (recommended if specific connectors such as Salesforce, HubSpot, GitHub are chosen) (#006). The answers are also injected into the Bot's context (#119). Quality 4: strong activation design; template copy is marketing.
- **Primary Bot** as personal chief of staff that "checks in proactively and coordinates work with your other Bots" (#002). A Team Bot cannot be primary.
- **Team Bot setup is a conversation with the Bot**: 99 scripted lines walk through plugins, secrets, skills, memories, routines and files, narrate what was added, and end with publish and an offer to add itself to Slack (#005). Deterministic, localised, not model-generated.
- **Personal to Team conversion** with an explicit carry-over checklist and a private-by-default rule for memories, chats and routines; `CloneGrokBotAgentToTeam` supports in-place or copy, and `PromoteGrokBotMemoriesToTeam` returns `created`, `already_in_team` and `kept_private_count` (#004). **Best-in-class** among all references for personal/shared boundary UX.
- **Draft, publish, republish, unpublish** lifecycle with "unpublished changes" diff label and auto-publish (#132).
- **Avatars**: shape x colour characters, uploaded photo, AI-generated avatar; per-Bot colour tints 1:1 messages (#115). Non-human avatars avoid impersonation.
- **Per-Bot voice, speed, language** (#127).

### 3.2 Personal vs team agents and access (2, 7)

One agent definition, many sessions: owner main chat, teammate DMs, Slack and Teams threads; **shared setup, per-person credentials** ("each teammate signs in with their own account, so no one shares a login") (#003, #045). Access: owner, manager, collaborator roles; users, groups, team row, link access; `expected_state_token` optimistic concurrency on ACL edits; "limit to listed people" while owner and managers always retain access (#065). Admin side: 41 `Admin*` RPCs, bulk member-computer kill/recreate/delete, setup manifests per user, team and organisation, access-state blockers (not offered / team access required / paywall...) (#066, #067, #096). **Risk disclosed by the vendor:** teammates' sessions can share one Bot computer, so shell commands added to a Team Bot's MCP setup run where teammates can also run a shell (#134).

### 3.3 Rooms: group chat of Bots and people (7)

- A group chat is created from the "To:" recipient bar (#112). Rooms are agents of kind ROOM; people are added by user id (#007).
- **Turn protocol** (descriptors only): each member is asked per batch of new messages (`RequestGrokBotRoomMemberTurn`) with peers, new messages, `is_winding_down`, a deadline and `parent_request_id`/`root_parent_request_id`; the member answers `SENT | PASS | SKIPPED | TIMEOUT | CANCELLED | ERROR` (#008). The lineage ids and winding-down flag are the vendor's loop guard. Who calls these RPCs is not visible (INF: server or Box host; medium).
- **Per-member phases** reading / typing / working with sticky commit so a Bot that has started acting does not flicker back to reading (#009, #129).
- **Cost guardrails**: usage notice, only shared Bots can join, local-machine Bots cannot; a remote-config experiment even tests hiding "create group chat" (`sand_group_chat_discouragement_2026_09`: control / notice / notice_and_hide_create) (#010, #105). Usage is attributed to a separate **Group chats** flow in the usage breakdown (#047). Signal: the vendor itself finds multi-agent rooms costly and is testing discouragement.
- **Bot-to-Bot messaging** with idempotent `message_id` and delivery outcomes (`FORBIDDEN`, `BOX_UNREACHABLE`, `TARGET_NOT_FOUND`...), rendered as a collapsed "messaged N Bots" row with a transcript view (#011).
- Corrections to the scout inventory (verified here): `work_overheard`, `work_landed`, `stay_silent` belong to the **voice-call harness**, not to rooms; `too_many_pings` is HTTP/2 GOAWAY handling.

### 3.4 The Bot's computer, live view and takeover (7, 12)

- **Box lifecycle**: run states, wake on demand, update scheduling with a catch-up window, recover/reset from snapshots with explicit "what is kept vs lost" copy, disk pressure SOFT/HARD handled by a built-in "Disk Saver" Bot flow (#015, #017, #018).
- **Pre-thaw**: the client reports intent signals (app foreground, chat open, composer focus, typing, upload start, file open, computer open) so a hibernated Box can be warmed **before** send; actions `PRETHAW | IN_FLIGHT | REFUSED` (#016). No reference app does this; it is cheap and generally applicable.
- **Live view**: noVNC viewer with preview vs interactive modes, multi-screen switcher, pause when the user is away, wake/retry states, liveness stall detection (SAND-E0609) (#019, #116).
- **Takeover and hand-back**: the Bot raises a hand-off (`box_handoff_request_id` plus instruction), the user clicks Take control, drives the desktop, presses "I'm done" (or closes the viewer, or dismisses); `EndGrokBotBoxHandoff` records the trigger and the Bot resumes (#020). A separate **awaiting** marker per browser tab covers sign-in pages (#021). Bot-detection walls (CAPTCHA, Cloudflare) are classified as a first-class outcome (SAND-E0417) (#121).
- **Teach by demonstration**: record yourself on the Bot's computer; the recording is turned into a message with initiator `TEACH_RECORDING` and the Bot learns the steps (#022).

### 3.5 Working on the user's own computer (7, 11)

The desktop registers as a **machine** and subscribes to a server stream of request frames (`exec`, `upload`, `download`, `local_tool`, `messages_op`, `discover_ai_tool`, `retire_approval`, `cancel`), with polling fallback and heartbeats. Every frame carries `approval_id` plus `authorized_by_standing` / `authorized_by_approval`, and the desktop may answer "permission pending", so authorization is **enforced on both sides** (#023, #100). Tools: shell with sandbox policy and bash AST classification (#035), file read/write/list, send input to a running command, Messages (macOS), a macOS computer-use helper (#024). A **phone** registers as a machine kind with `mobile_local_tools` (#025). Egress can be routed through the desktop for IP-sensitive sites unless an admin disables it (#097). Quality 5 for the frame/provenance design; **Windows lacks the local computer-use helper, Chrome cookie import and the desktop mount** (section 6).

### 3.6 Approvals, rules, secrets, purchases (7, 10)

- **Permission policy** per tool and per MCP server, three states, admin ceiling NEVER/ASK/ALWAYS (#026).
- **Cards**: allow once / always (this Bot, or all Team Bots in the org, with a duration choice) / deny once / never, Esc denies; cards reconcile across devices ("decided elsewhere; card is catching up") (#027).
- **Auto-review**: a model reviews risky actions; users write up to **20 natural-language rules of up to 1,000 characters**, team rules always apply, built-ins always apply, "Ask first" wins conflicts; rule changes appear in the transcript as **word-level diffs** (LCS) with "replaces" semantics; rule text is scrubbed of query strings and tokens (#028, #029). **Best-in-class** for usability of policy.
- **Addressed approvals**: waiting states name the person ("Only X can answer this", "uses X's account, never yours") and show interruptions (#030).
- **Credentials**: 1Password with vault-scoped service accounts, per-request approval, fill into the Bot's browser, never into the model (#031); Secure Forms with origin-bound vault entries (#032); write-only team secrets as env vars (#033); cookie sync with per-origin approval on macOS/Linux (#038).
- **Purchases**: virtual card via Stripe Link, passkey assertion, explicit "no charge was made" terminal states (#034); passkeys and hardware keys, with a desktop consent window that shows origin and relying party for tunnelled WebAuthn (#037).
- **Standing grants are reviewable and clearable** (#136).

### 3.7 Scheduled and event-driven work (7)

**Routines** are created by asking the Bot, edited in a panel (when to run, instruction, pause/resume, test, delete). Triggers: schedules (seconds to years, weekdays, cron), webhook (bearer key, test), email arrival (optionally including unauthenticated senders), Slack and Teams messages, mentions, reactions, calendar events, GitHub, Linear, Sentry, PagerDuty (#039, #040). Routines can be personal or live on the team copy; they are **paused when the user is away** to avoid wasted spend (#041). Events wake Bots with a **durable pending-wake ledger**; cloud-agent watchers register `LAUNCH | REPLY | WATCH` (#042).

### 3.7a Memory, skills, plugins, usage (7, 8)

Per-person facts and promotable team memory (#043), skills with mandatory discoverability description (#044), plugins with variables and per-person accounts (#045), model choice (#046), **usage attribution by app, flow (user / automation / Bot-to-Bot / group) and Bot**, weekly included usage, on-demand spend limit, prepaid auto top-up, and failure causes by paying account owner vs acting user (#047). Tasks have WORKING / NEEDS_INPUT / DONE and task threads (#013) [category 8], subagents and an async tasks panel (#014).

### 3.8 Calls and meetings (3)

- **Voice chat** is a two-layer system: a realtime voice persona (WebSocket events `session.update`, `input_audio_buffer.append`, `response.create`...) that speaks, and the Bot that works. The persona delegates through `send_task` (requests capped at 2,000 characters, with a refusal telling the model not to drop prohibitions or approval requirements), `recall_text_messages`, `search_conversations` (scope this-chat / earlier / everything; `if_missing` say-no-record or send-task), `stay_silent`, `end_the_call`; Bot progress is injected as `work_landed`/`work_overheard` updates; the harness measures mic route changes, barge-in drops and dead lines (#050, #051). **Best-in-class** and directly reusable.
- **Phone calls to third parties** need an approval card that states the call cannot be recalled (#052).
- **Meetings**: Bots join Google Meet and Zoom links as visible guests with avatar; consent per meeting or for all; meetings visible to managers or everyone; meeting chat drafts; versioned summaries (admin) (#053, #054). [SRC; join behaviour not observed.]

### 3.9 Messaging, notifications, search (1, 4, 5)

- Send journal, echo confirmation, UNKNOWN_DURABILITY, typed error codes (#056); session kinds (#057); threads and task thread pane (#058); composer with @Bots, People, Groups, Routines, Plugins (with live status), Skills, Actions (#059); voice memos with word-level transcript jump (#060); reactions that can wake Bots, feedback votes (#061); entry-anchored unread (#062); typed previews and rich cards (#063); delivery uncertainty states for external sends (#064).
- Notifications: per-Bot "finished or waiting for you" (#074), permission reporting and active-device gate (#075), attention queue (#076), Bot-delivered product announcements (#077, medium).
- Search: command palette and Bot switcher; retrieval delegated to the Bots (#071, #072); **no global human message search found**.

### 3.10 Extensibility (6)

Slack app **per Bot**, created programmatically with the complete admin-approval state machine (#078); Teams app per Bot with tenant, team and admin-approval handling (#079); templates with share links, versions and an export policy (#080); MCP with per-person OAuth and per-tool permissions (#081); per-Bot email inboxes (#082); cloud agents and PR chat (#083); widgets and forms (#084); identity linking Slack-to-account (#137); per-user runtime settings (#138). Outbound Bot posting into Slack channels and Discord is labelled "coming soon" (#128).

### 3.11 Client shell and UX patterns (11, 12)

Command registry of 17 ids with hotkeys and a palette (#106 area, #107); update tracks and "no update while an agent works" (#104); Windows parked-installer update path (#130); coded error registry with 108 `SAND-E` codes across 8 domains with retryable flags and source anchors (#120); Network Debugger for enterprise proxies (#108); 260 remote-config entries including A/B experiments (#109); sidebar IA (pinned, sections, hidden, batch, context menu) (#111); New chat as recipient bar (#112); onboarding steps meet / computer-demo / jobs / role / tools / create / notifications / hand-off (#113); plain-language activity and trust copy (#114); avatars (#115); computer view UX (#116); honest failure states (#117); usage meter with thresholds (#118).

(Record numbers above reference the feature file in order; use the file's `name` field to look up a record by topic.)

### 3.12 Resume-pass additions (#139 to #171)

The resume pass looked for gaps by (a) listing English UI messages that no record cited (3,456 of 3,700, mostly variants of topics already covered), (b) listing protocol RPCs no record mentioned (215 of 484, mostly admin, marketplace-operator and store plumbing for concepts already recorded), and (c) listing the 343 semantic component class names in the renderer (`sand-*`), which form a map of UI parts. Component-level code reading produced the areas below, which the first pass had not captured. All are [SRC] unless stated.

- **Latency hiding in the send path (#139, #140).** While the user is still composing, staged files are pre-uploaded to the object store (10 second release timer, cap of 6 concurrent, hold and take at send) and staged images are pre-captioned by the server through a debounced, deadline-bound, backoff-aware scheduler that passes draft text and recent user messages as captioning context. This is the same idea as the Box pre-thaw (#016): spend idle composing time to remove cold-start cost. Attachment limits come in two tiers (inline path 25 MiB per file and 200 MiB per video; object-store path 25 MiB images, 250 MiB other files, 1 GiB video) with typed refusal reasons.
- **Composer and message surfaces (#141 to #146, #166).** The composer is a Tiptap/ProseMirror editor that deliberately disables formatting marks (no bold, italic, code or headings), leaving paragraphs, links, mention chips and lists; it adds automatic text direction, an ARIA placeholder, caret scrolling, macOS Emacs-style Ctrl-A and Ctrl-E, emoji shortcode completion with an IME guard, and mention scoping by conversation type. The hover toolbar offers react, download, copy and open-page actions; **no edit or delete of a sent message was found** (medium confidence), consistent with the product being agent-centric. Link previews are fetched in the main process by an SSRF-safe fetcher (#145: HTTPS only, public hosts only, DNS resolved and the connection pinned to the checked address, redirects re-validated, deadlines and byte limits) and cached in the renderer under an 8 MiB budget that sheds large images first (#144). Unknown or retired message types render as an explicit card with once-per-type telemetry (#146).
- **What the agent is doing and what went wrong (#147, #150, #152, #165, #167).** The transcript carries a closed vocabulary of 27 system-event keys (#147) that doubles as a checklist of agent wake and resume reasons: host update, box handoff, MCP authentication, listener connect, secret submission, user form submission, reaction, draft send, background task and command follow-ups, delivery failures, routine sync. Activity phrases are chosen from 20 kinds with rotating variants (#150). Drafted outbound messages become editable cards whose revisions highlight what the Bot changed, fading after 10 seconds, with an explicit unconfirmed-send state (#152). Computer lifecycle operations show named steps and a progress bar as an accessible ordered list (#165). Bot creation uses a client-generated id the server must echo, plus an optional kickstart introduction in the user's language (#167).
- **Layout and accessibility (#148, #149, #158 to #163).** The transcript is typed rows with synthetic group rows for attachments, agent-to-agent communication, routine events and cloud-agent activity (#148); rows are positioned absolutely and reaction rows animate sibling shifts (#149). Accessibility is better than the first pass assumed: a polite and an assertive live-region announcer (#160), and pane transitions that make the outgoing pane inert, move focus to the new pane's heading and skip animation under reduced motion (#161). The details pane lingers 300 ms on close and hands focus off (#162); overlays share a top-layer popover root (#158); holding Command or Control for half a second reveals numbered sidebar shortcuts (#159). No high-contrast or forced-colors handling was found.
- **Client shell and start-up (#154 to #157, #169 to #171).** A native menu with an update-state item and a native unread-only filter toggle (#155); platform-specific window chrome with a zoom-aware inset (#156); a badge count projected from the roster with epoch and sequence guards that skips hidden and muted Bots (#157); a deep-link router for Bot, task thread and create-Team-Bot (#154). Start-up is a staged lifecycle with a 30 second stuck detector and a coded retry (#170) and a first-run gate that derives whether onboarding is needed from sign-in, a saved flag, roster size and computer creation, failing open on slow checks (#171). Voice calls have an explicit state machine with a call control that follows the user to other Bots and a recoverable-close policy (#169).
- **Team Bots are gated (#153).** Publishing a Bot to the team needs the multiplayer rollout flag, a team membership and an agent that runs on the durable server harness; other agents cannot be shared. Provider-aware link cards for external documents (#168) show that the product treats Notion, Google, Figma, SharePoint and similar documents as typed references rather than owning them.

## 4. What is best-in-class (within the agent-product facet)

Relative to the other references named in the brief (Slack, Teams, buzz, qm) the comparison is INF: those apps are audited by other facets, and I only know their feature classes in general.

| Area | Grok Bot assessment | Why |
|---|---|---|
| Personal agent with its own computer + live view + takeover + hand-back | Best-in-class (#015 to #021) | Complete loop: wake, watch, pre-thaw, take control, hand back, awaiting-user marker, bot-block detection |
| Approval ladder and policy UX | Best-in-class (#026 to #030, #100, #136) | Scope ladder, multi-device reconciliation, NL rules with diffs, addressed approvals, provenance on every local effect |
| Personal to team conversion with privacy-by-default | Best-in-class (#003 to #005) | Carry-over checklist, private memory triage, scripted narrated flow |
| Agent as a first-class external app (Slack, Teams) | Best-in-class (#078, #079) | Programmatic app creation, admin approval, reinstall, move, forget, removal state machines |
| Voice front and Bot back, with constraint-preserving handoff | Best-in-class (#050, #051) | Realtime persona delegates through a typed tool with prohibitions preserved |
| Push transport with exact resume (updated_seq, generation, cursor_too_old) | Strong (#088, #089) | Better than append-only logs; enables rollback and edits |
| Send reliability (journal, echo, UNKNOWN_DURABILITY) | Strong (#056) | Honest about what is not known |
| Routines and wake ledger | Strong (#039 to #042) | Chat-authored, broad triggers, away-pause |
| Coded error registry with retryability | Distinctive (#120) | Maintainable at scale |
| Latency hiding while composing (pre-upload, pre-caption) | Best-in-class (#139) | Cancel, commit, hold, timer release and backoff are all handled |
| Accessible navigation and live regions | Strong (#160, #161) | Inert outgoing pane, heading focus, reduced-motion bypass, polite and assertive announcer |
| Draft hand-off with revision highlights | Strong (#152) | Humans see exactly what the Bot changed before sending |
| Closed system-event vocabulary for agent wake and failure reasons | Distinctive (#147) | Makes every resume path visible and localizable |
| Human-to-human collaboration | Absent | See weaknesses |

## 5. Weaknesses and limitations

1. **Humans are second-class.** No human DMs, channels, edits, deletes, message search, huddles or screen share. Group chats are agent-centric and cost-discouraged by the vendor's own experiments (#010, #105). [SRC absence, medium]
2. **Cost and trust of multi-agent rooms**: every member Bot processes every message; a turn protocol mitigates but does not remove it (#008, #010).
3. **Shared computers across teammate sessions** (#134) and cookie sync (#038) widen blast radius; both are disclosed, neither is solved.
4. **Platform skew**: desktop mount, Chrome cookie import, Messages and local computer-use are macOS or Linux only; 1Password auto-setup needs a Mac (#107, #038, #031, #024, #105 platform gates). On Windows the product is mostly the chat, shell and file tools.
5. **Vendor lock-in** to the Cursor platform (auth, billing, hosts) and to the Box runtime; the Temporal harness migration shows the runtime is still in flux (#092).
6. **Search** is weak (no cross-conversation message search found).
7. **Routines are only as auditable as the Bot**: creation is conversational; no schedule review UI beyond the panel was found.
8. **Unverified server behaviour**: room orchestration, Auto-review reviewer, meeting join, memory triage, voice persona prompts and Box runtime run server-side and were not observed.
9. **Disclosed unknowns**: what the GLASS surface is (#025), how teach recordings become skills (#022), who calls room-turn RPCs (#008).
10. **No edit or delete of sent messages** and no message forwarding were found in the message toolbar (#143); formatting marks are disabled in the composer (#141). Reactions exist (#143, #061) but only inside Bot conversations.
11. **Link unfurling runs in every desktop client** (#145), duplicating a security-critical guard per device instead of one audited service.
12. **Accessibility coverage is partial**: strong focus and live-region work (#160, #161), but few explicit aria labels in the main bundle and no high-contrast or forced-colors handling were found.
13. **REA could not recover the semantic graph of `main-app.cjs`** (call-stack limit) and `main-core.cjs` (heap limit), so main-process findings in those files rest on direct source reading, not on REA traces.

## 6. What Verevon should copy, adapt or avoid

Verevon's baseline (from the two baseline files and `CLAUDE.md`): authority-first rooms where every post is a model turn; no personal agents; no human-only message path; polling at 6 s; approvals owner-bound with one scope and no resume; agent PC/browser partial; no live view; no notifications from Space events; no Space memory or cost; schedules only creatable in Settings; watches not exposed; Channel Plane docs-only. The constraints (no cross-plane DB access, content-bound append decisions, deny-by-default effect policies, ZDR propagation, Space access never widens an owner ACL) apply to every adoption below.

**Copy (high value, fits the plane model)**

| # | Pattern (record) | Verevon gap it closes | Plane owner / constraint |
|---|---|---|---|
| 1 | Approval scope ladder, "card catching up" reconciliation, addressed approvals, Esc deny (#027, #030) | Single approval scope; owner-bound; no resume | Model Plane approval scope first (their own blocker); Control effect decisions bind each scope |
| 2 | NL Auto-review rules, user over team over built-in, diff notices (#028, #029) | No policy UX; binding policy is back-end only | Rules belong to Control (org/team) and Space bindings; reviewer in Model Plane |
| 3 | Authorization provenance on every effect request (#100) | Receipts exist but endpoint-side proof is missing | Quarry and sandbox-manager request frames |
| 4 | Multiplexed sequenced stream with `updated_seq`, generation, `cursor_too_old`, heartbeats (#088, #089) | 6 s polling, no push, no exact resume | Application plane projection fed by Session Core; per-entry audience filtering still required |
| 5 | Live state with `stale_after_ms`, awaiting reason, hand-off request (#012, #020, #021) | Opaque agent work in room | Model Plane run events to Application projection |
| 6 | Persistent per-agent computer with hibernate, pre-thaw on composer focus, live view, take over / hand back (#015 to #020) | Agent PC/browser partial; no live view | sandbox-manager durable workspace (their S3.3) plus Quarry browser grants; ZDR and egress policy must propagate |
| 7 | Personal to team conversion with carry-over checklist and private-memory triage (#004) | Personal Space cannot hold agents; no memory scope | Control owns membership; memory scope needs a plane decision (Model vs Data) |
| 8 | Durable agent identity separate from sessions; session kinds (#001, #057) | Agents only exist inside rooms | Control principal for the agent plus Application binding |
| 9 | Needs-attention queue and per-Bot finished/waiting notifications with active-device routing (#074 to #076) | No Space notifications | notification-core allowlist and delivery ledger (S4.4) |
| 10 | Routines created by chat, broad triggers, away-pause, test run, wake ledger (#039 to #042) | Schedules Settings-only; watches unexposed | capability-core watches plus Control schedule decisions |
| 11 | Voice persona delegating via a typed tool that preserves prohibitions (#050) | Realtime is AI-only | Model Gateway realtime plus approval gating |
| 12 | Per-agent Slack/Teams app with full install state machine (#078, #079) | Channel Plane docs-only | Channel Plane S5.1 to S5.3; model every failure state listed |
| 13 | Pre-upload and pre-caption attachments while composing, with a release timer (#139, #140) | Attachments are processed only at send | Data v2 ingestion and Ingestion plane; unsent-file lifetime must obey ZDR |
| 14 | Closed system-event vocabulary and wake-reason checklist (#147) | No typed agent failure or resume events in Space | Application plane event types fed by Model Plane run events |
| 15 | Editable draft cards with revision highlights and unconfirmed-send state (#152) | Outbound agent messages have no human review card | Model Plane approval plus Channel Plane drafts; Control audit on send |
| 16 | Accessible pane navigation, live-region announcer, popover overlay root (#160, #161, #158) | No stated accessibility foundation for Space | Frontend Plane (Verevon v3) shared primitives |
| 17 | SSRF-safe link unfurl (resolve, pin, re-validate redirects) and bounded preview cache (#144, #145) | Link previews would otherwise fetch from clients or unguarded services | Ingestion plane Quarry fetcher with tenant-partitioned cache |
| 18 | Staged start-up lifecycle with a stuck detector and derived first-run gate (#170, #171) | No observable restore failures; onboarding is not derived | Frontend Plane session bootstrap |
| 19 | Row grouping for agent chatter, routine and cloud-agent events (#148) | Rooms flooded by machine activity | Application plane projection and Frontend row model |

**Adapt (good idea, needs care)**

- Multi-agent turn protocol with PASS, deadline, lineage ids and winding-down flag (#008) plus status hysteresis (#129): adopt the contract, but have the room orchestrator in Application or Model Plane enforce the depth cap and initiating-human chain from their UI-2c plan; show cost at room creation (#010).
- Local-machine reverse channel (#023): adopt the frame vocabulary but design the Windows-native path first (Aquatiq's estate is Windows); require explicit device enrollment and admin ceilings (#026).
- Credential flows (#031 to #034): use a broker pattern; purchases and virtual cards only after an effect-bound passkey step exists.
- Coded error registry (#120) and retry semantics: start with a shared registry across gateway, session-core, quarry.
- Templates and role/tool recommendations (#006): ship a small curated set per Verevon vertical (food-safety, hygiene, process) rather than 74 generic ones.

**Avoid**

- Letting teammate sessions share one execution environment by default (#134); keep one sandbox per human principal inside a Space agent unless explicitly shared.
- Cookie import from the user's browser (#038) as a default auth path; if ever offered, per-origin consent, audit and ZDR-aware handling.
- Making every member agent process every room message by default (#010); require explicit mention or turn grant.
- Copying platform-specific helpers (macOS Messages, local computer-use) as core features.
- Deriving Verevon's human-collaboration design from Grok Bot: it offers none; use Slack, Teams, buzz and qm for that half.
- Running link unfurling in every client (#145); do it once in a hardened service.
- Disabling all composer formatting and message edit or delete (#141, #143) if Verevon wants Teams or Slack parity for human conversation.

## 7. Open questions (for a runtime or server-side follow-up)

1. Who calls `RequestGrokBotRoomMemberTurn` and where do the depth cap and winding-down flag get decided (#008)?
2. How is `Auto-review` implemented (reviewer model, prompt, latency) and what are the built-in rules (#028)?
3. What do teach recordings become (video, event log, skill markdown) (#022)?
4. Does any human-facing message search exist server-side, and is audit of admin transcript reads recorded (#066, #072)?
5. What does GLASS mean (#025), and does Windows ship a local computer-use path (#024)?
6. How are per-person private memories stored and isolated on a shared Team Bot computer (#043, #134)?
7. What are the sandbox enforcement mechanics on Windows given no sandbox binary ships under dist (#035)?
8. What does the server do with a pre-caption request (#139), and are unsent pre-uploaded files deleted server-side when the 10 second client timer fires?
9. Where is the stable per-turn fraction for activity phrase rotation seeded (#150), and what is the rating-only call mode (#169)?
10. Does any edit or delete of a sent message exist on mobile or server APIs even though the desktop toolbar has none (#143)?

## 8. Corrections to the scout inventory (`grokbot.inventory.md`)

- `outbox.v1.jsonl` is the **Dune telemetry outbox** (log rows keyed `dune`), not a message outbox. Send reliability lives in the renderer/coordinator **send journal** and persistence slices (#056, #090).
- Coordinator tokens `work_overheard`, `work_landed`, `stay_silent`, `turn_ended` map to the **voice-call harness**, not to rooms (#050). `too_many_pings` is HTTP/2 GOAWAY handling.
- The "typed team secrets" list (Slack token, GitHub token, Google API key, Entra ID, email, CLI credentials, generic) is the **client-side redaction pattern set** (`<REDACTED: label>`), not a secret-type picker (#133, #033).
- `CURSOR_MDM_SIGN_IN_POLICY_JSON` was not found as a literal in dist; the device-policy sign-in restriction exists as UI copy and error SAND-E0218 (#099).
- Chrome cookie import and the desktop mount are **darwin/linux only**; Windows has neither (#107, #038).
- The Windows update path uses a **parked installer** (#130); this install is Windows.
- Message counts: English catalog about 3,700 messages (the scout used a 57k-string multi-locale candidate list).
- Resume pass: renderer chunk names such as `chunk-chat-*` and `chunk-core-*` that look like feature code are mostly **per-locale translation catalogs**; feature code lives in `index-Bo3NzkuN.js`, `index.eager-app-*.js` and the named view chunks.
- Resume pass: `chunk-use-draft-revision-highlight` contains the draft revision diff and the approval-mode logic together (#152).

## 9. Corpus statistics (feature file)

171 records (138 from the first pass, 33 from the resume pass): Agents in the room 59; UX patterns & layout 19; Messaging core 16; Client shell 14; Realtime & sync architecture 11; Org & membership 9; Apps & extensibility 9; Governance, security & compliance 9; Collaborative artifacts 8; Calls & meetings 8; Notifications & attention 5; Search & discovery 4. Provenance SRC 168, REA 3, DOC 0, INF 0. Confidence high 140, medium 31, low 0. Quality 5: 16, 4: 108, 3: 46, 2: 1. Uniqueness unique 44, uncommon 75, common 52. Quality is relative maturity and execution judged from descriptors, code paths and UI state coverage, not runtime behaviour.
