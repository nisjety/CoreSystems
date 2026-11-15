# Manus Studio 2.0.6 - product and UX deep dive (facet: product-ux)

Audit 3 (Model Plane + Verevon chat and agent pages). Read-only. Source: Manus desktop 2.0.6 (MSIX, Windows). Companion file: `manus.product-ux.features.json` (130 records, ids manus-product-ux-001 to 130). This narrative explains the architecture, the data model, the design decisions, what is best-in-class, what is weak, and what Verevon should copy or avoid.

Provenance tags used: [SRC] read directly in shipped files (renderer chunks, TypeScript, JSON, dist JS), [DOC] documentation text shipped inside the app (addon SKILL.md files and manifests), [REA] observed through the REA analysis tools, [INF] my inference with the basis stated. Confidence is "high" unless a line says otherwise.

---

## 0. Scope, method and honest limits

What I analysed
- The scout inventory (`manus.inventory.md`) was read first and used for orientation only; every claim below that I rely on was re-verified in the files.
- The renderer is a minified Next.js static export (`extract/manus/frontend/out`, 124 MB, 648 chunks). There are no source maps. I extracted three kinds of signal from it: (1) the English i18n catalog (24 locale chunks of 11,508 entries; the English source strings are the keys, so one locale chunk is a full UI string dump), (2) readable application code in the two big app chunks (`0sydbzkqidpbv.js` 8.3 MB and `0h7s-hxz9tj9m.js` 11.4 MB, plus the store module `2jwv7cz4lh77_.js` and constants module `0ygo416gxr4ri.js`), where Redux slice definitions, selectors, component logic and l10n call sites survive minification, and (3) 178 embedded protobuf file descriptors (`fileDesc("...")` base64 blobs) that I decoded with a small wire-format reader. The decoded descriptors give the entire API surface: 167 first-party proto files, 64 packages, 144 services, 1,428 RPC methods. This is the most reliable view of Manus's data model that is available without the backend [SRC].
- The managed addon archives (`resources/managed-addons/*.tar.gz`) were extracted into scratch (not into the install directory) and their SKILL.md files and manifests read. These are shipped instructions for the agent, so they document how Manus actually wants the agent to behave [DOC].
- The two workspace TypeScript packages `@next-agent/browser-runtime` (50 src files) and `@next-agent/screen-live-service` (57 src files) were read directly.
- REA was really used on the two workspace packages (see section 9).

What I could not do
- No app was launched and no backend was contacted, so server behaviour, ranking, quotas, and feature flags are invisible. UI strings tell me a feature exists; they rarely tell me how well it works.
- Many features are plan-gated or region-gated (strings mention "available only in the United States", "not available in your region"). Presence in the bundle is not proof of general availability.
- Peer comparisons ("no peer has X") are [INF] unless stated; I did not re-audit the other tools in this pass.
- No per-user data directories were read. Secret-bearing files (browser data import, device credentials, localStorage.json) were not opened.

Context worth knowing: UI strings in this build mention that Manus "prepares to resume operations as an independent company" and describe scheduled data deletion, account and task-data backup, and a restore period [SRC]. Some export and restore flows (record 019) therefore reflect a one-off corporate transition and are not general product patterns.

---

## 1. What Manus is, as a product shape

Manus presents itself as an "action engine": a cloud (and optionally local) agent that does a task end to end and hands back deliverables. The shipped UI shows a much wider surface than a chat app [SRC]:

| Surface | What it is | Key records |
|---|---|---|
| Tasks (chat) | One conversation per task, a plan dock, a message queue, a right-hand "computer" panel | 001-006, 062-064, 119, 127 |
| Wide Research | Parallel subagent fan-out with a cost gate and result grid | 038, 045, 128 |
| Projects, Library, Sharing | Server-side projects with instructions, roles, files, shared tasks; cross-task file library; replay sharing | 008-018 |
| Automations | Scheduled tasks plus event, webhook and script triggers; run history | 049-052 |
| Agents ("Cue") | Persistent named agents with email, phone, payment, computer, skills; groups and channels | 054-055 |
| Creation studios | Slides, video, images and canvas, audio, web/app builder, meetings | 022-037 |
| Computers | Cloud computers (Linux, Windows, macOS), My computers remote control, Work Locally, My Browser | 064-066, 076-080 |
| Extensibility | Skills, addons with widgets, custom MCP, tunnels, public API, Slack and IM | 089-096 |
| Governance | Teams, data classification, connector and network policies, SIEM streams | 070-074, 086 |

Naming facts from the constants module [SRC, `0ygo416gxr4ri.js`]: the quality tiers are "Manus 2.0 Lite", "Manus 2.0" and "Manus 2.0 Max"; the agents product is called "Cue" (PRODUCT_NAME_AGENTS, cue.im); other product names are "Manus Office", "Manus Pro", "Manus Team" and "Manus Flex" (bring-your-own-model). UI strings use a `{product}` placeholder instead of the literal name, so one catalog serves several brands.

Chat layout constants [SRC]: session list 52 px folded or 300 px open; chat column 400 to 768 px; right-hand dashboard minimum 520 px; the web-dev sidebar breakpoints are 560 and 616 px.

---

## 2. How the task experience is built

### 2.1 State model: an event-sourced stream rendered through "scopes"

Everything in a task is an event with an id, type, timestamp (client-created events use `Date.now()`, so milliseconds) and often a revision. The store (`websocketSlice` in `2jwv7cz4lh77_.js@980168`) upserts events through an entity adapter and handles [SRC]:
- deltas: `isFrontendEventDelta` events target an earlier event id and append `delta.content` and `delta.thought` to a pending assistant message (record 001);
- revisions: a `referencedEventUpdate` with an older revision is dropped;
- gap fill: `missingEventsReceived` merges events that were missed;
- windows: pending jump anchor and older/newer boundary ids, first and last segment start ids, `hasMoreSegments` (record 109);
- special events that are not rendered: `desktopSessionConfig`, `desktopDevicesUpdate`, `deviceWorkspaceGitStatus`; context events (`contextTooLong`) update a context-length status.

Which panel shows an event is decided by `FrontendEventEffectScope.isEventInScope(event, "timeline" | "chatBox" | ...)`. The same events therefore feed the chat column, the computer panel, the timeline scrubber and the shared replay page without separate data paths (record 115). This is the architectural reason replay is cheap for Manus: replay is the live renderer run over stored segments (record 009).

Event types seen in render code [SRC]: chat, toolUsed (with a `tool` kind and `getToolUsedDetail`), statusUpdate (`agentStatus` running, waiting, stopped, error), stepGroup, taskReference, sessionForked, followUpEnded, newTask, workspaceDirectoryAccessResult, addonWidgetCard, addonResourceDisplay, referencedEventUpdate, contextTooLong, deviceAttachmentUploadNotify.

### 2.2 Transport

Chat runs over a socket.io WebSocket (`WebsocketHelper`, `chatWebsocketUrl`). On connect and on every reconnect the client emits `join_session` with `version: 2` and `lastMessageId` (the last known join point), then rehydrates cascade jobs (`hydrateCascadeJobs({reason: "reconnect"})`). Client actions are typed messages through `chatWebsocketActionsController.messageSentToSocket(type, payload)` [SRC, `0h7s-hxz9tj9m.js@751950`] (record 129). Observed types: join_session, leave_session, stop_session, user_message, interrupt, steer, message_queue_action, change_task_mode, change_local_permission_mode, start_sandbox, kill_cascade_job, kill_cascade_job_group, retry_map_reduce_subtask, resume_map_reduce, force_continue_map_reduce, set_workspace_working_directory, switch_deploy_version, plus a family of confirmations (section 3.4). Product APIs use Connect/protobuf. Notifications use a separate notifier-v2 socket.io path. Desktop-local features go through an Electron IPC bridge and a local operator process (covered by the scout).

### 2.3 Panels and tabs

The task view is a chat column plus a right-hand area whose content type is `agentWorkspaceSlice.sidebarType` (workspace, filePreview, canvas, video, webdev, scheduled, trigger, cascadeJobs, chatReplySources, addonWidget) [SRC `2jwv7cz4lh77_.js@309542`]. A separate tab model (`sessionSwitch`, `@403060`) holds several tabs per task: newTab, filePreview, cascadeJobs, browser, terminal and webdev or gamedev; some are singletons; tabs can be reordered and are restored after restart; before the first task exists the tabs live under a `project:<id>` or `new-task` pseudo-session (record 108).

### 2.4 The planner, the queue and the composer

- The planner and the message queue share one card docked to the composer (`eyE` in `0sydbzkqidpbv.js@2055710`): one folded line with the current step, a `Step N/M` toggle, and an expanded "Task progress" list; it fades with AnimatePresence and skips animation under reduced motion (record 004). Queue states: cap reached, paused because the user stopped the task, paused because the agent awaits input (record 003).
- The send control is a split button whose primary action depends on state. While a task runs, the dropdown offers "Interrupt and send", "Send without interrupting" (steer, `onSteer`) and "Add to queue" (`onQueue`); the choice is remembered under `running_task_send_preference`. Stopping while subagents, video or speech run raises a typed confirmation (records 002, 058).
- A separate "plan mode" exists: the agent calls `submit_plan`; the card is backed by a Markdown file (`planFile`, `planDownloadUrl`), shows "Awaiting confirmation", and ends as "Plan executed on ..." or "Plan rejected on ...". Only the latest pending plan is actionable; read-only viewers cannot act; edits and confirmation failures are handled (record 119). Web-dev builds use a different "Review feature plan" editable list.

### 2.5 The computer panel, timeline and replay

The panel renders the selected timeline point with a viewer chosen by `toolStatus.tool` [SRC `0sydbzkqidpbv.js@2039315`]: browser, webpage_extract, search, search_scholar, search_image, terminal, tasks, text_editor and webdev_debug, media_viewer, search_gmail, read_gmail, mcp, map_reduce. A one-line status says which tool is in use ("{name} is using Browser", "... Editor", "... Terminal", "... Scholar search", and so on) (records 062, 039).

Timeline logic (`timelineUtils`, `2jwv7cz4lh77_.js@762342` and `@762473`) is the most distinctive mechanism in the product [SRC]:
- Points are tool events of the kinds above, text-editor events that have a path (slides excluded), and addon widget cards; events with status `rollback` are excluded.
- Points get a compressed timestamp: gaps up to 3 hours are kept, gaps between 3 and 24 hours are scaled by 0.3, longer gaps collapse to 5 minutes, and every gap is at least 10 seconds. The slider percentage maps to the nearest compressed point; "virtual" points resolve to the closest real event in their segment.
- `isTimelinePlaying` drives playback; "Jump to live" returns to the latest state; `followLatestTimeline` and `getFollowAgentStatusByTimeline` keep the panel in sync with a running agent (record 063).

The shared replay page (`sharePage` slice, `@1007798`) starts a countdown of 3 seconds ("Replay will start automatically in N seconds"), shows a loading and a skip state, prefetches segments, and uses the same renderer. Each Wide Research subtask tile links to the sub-session with `replay=1` (records 009, 038). Replay is event re-rendering, not video. [INF] Its fidelity therefore depends on what each event stores (screenshots for browser steps, text for terminal and editor); I did not verify storage, retention or whether events are retained under the product's own privacy rules.

### 2.6 Takeover

Two paths exist (record 064):
1. Agent-suggested: an event field `suggestUserTakeover` is `browser` (cloud browser) or `my_browser` (the user's own browser). A card "You can choose to take over the browser to complete the operation" with a "Take control" button appears only on the latest event with no newer segments, and not for read-only viewers (`0sydbzkqidpbv.js@4061548`).
2. User-initiated: the VNC dialog (`vncDialogVisible`) is deep-linkable through a `vnc=1` query parameter and pushes browser history, so reload and back work (`2jwv7cz4lh77_.js@309542`).

The viewer first tries a WebRTC viewer in an iframe (the sandbox event carries `webRTCUrl`; credentials `username` and `webrtc_token` come from `sessionApi.getVncOtp`, which returns `{authEnabled, otp, nekoOtp}`) and falls back to a lazily loaded noVNC RFB client whose WebSocket subprotocols are `["rfb", otp]` (`@7696104`, `@1623039`). A footer lets the user choose "Save login cookies" or "Ask me to log in each time". After takeover the user is asked to describe what they did so the agent can continue. Credentials never touch the model because the user types into the live browser. For `my_browser` the app switches to the user's own browser tab through the extension, or asks the server to reselect it (`reselect_my_browser`).

### 2.6.1 The in-app browser runtime (desktop)

For tasks on the desktop, `@next-agent/browser-runtime` provides the agent browser (record 066, 067, 068). Design points that matter for Verevon's "model proposes, Quarry executes" rule [SRC, with REA corroboration of the module graph]:
- 22 MCP tools with annotations (readOnly, destructive, openWorld); a hybrid DOM and accessibility snapshot with cursor paging and `browser_find`; every mutating tool carries tab and document revision tokens; coordinate clicks are bound to a `captureId` and to the exact raster size the model saw.
- A typed error table where each code states `retryable` and `requiresSnapshot` (STALE_PAGE, STALE_REF, REF_NOT_INTERACTABLE, TAB_CLOSED_BY_USER, ACTION_REPLAY_CONFLICT, ACTION_LEDGER_FULL, ACTION_IN_DOUBT, ...). The last three imply an action ledger for idempotency.
- Sensitive input: payment fields are filled from operator-supplied temporary references into standard autocomplete targets (cc-number, cc-exp, cc-csc, address fields) through a staged pipeline (validation, target, origin, field, focus, clear, mask, insert, verify, select); trusted secrets may only be typed on their allowed host (same host or subdomain).
- Presentation: only the foreground task's tab is presented, background tasks never steal the visible surface; the PiP status carries `agentActive`, `lastAgentTarget.sequence` and tab provenance (`openedBy: user | agent`); a 640 px, JPEG quality 70 frame feeds the preview; retained workspaces live 30 minutes, at most 10 sessions and 12 tabs; restore points persist per tab (0600) and mark pages that tried to launch an external application so they are not auto-loaded.
- Diagnostics are timing-only JSONL with hashed ids, capped at 50,000 records (record 087).

---

## 3. Data model (from the decoded protobuf and the addon manifests)

### 3.1 Sessions, modes and files [SRC, `session/v1/session_base.proto`]
- `TaskMode` LITE/STANDARD/MAX; `AgentTaskMode` STANDARD/HIGH_EFFORT; `RunMode` ASK_USER/FULL_AUTO. Agent profiles in the tasks addon add `max-medium`, `max-high`, `max-extra`, `max-ultra` [DOC].
- `SessionCreationSource` USER, TEMPLATE, CLONE, SCHEDULED_TASK, MAP_REDUCE, PLAYBOOK, EMAIL, API, COORDINATOR, COORDINATOR_SUBTASK, CHANNEL, GROUP, FORK, TRIGGER: every task records how it began, which makes lists, billing and audit explainable (records 007, 060).
- `QueueType` DEFAULT, VIP, BUFFER, EDU_TEAM, SCHEDULE_FREE, SCHEDULE_VIP, LITE, STANDARD, MAX, LITE_FREE, STANDARD_FREE, with `GetTaskQueueInfo` returning wait estimates per tier (record 048).
- `BackgroundActivityType` NONE/AGENT/PROCESS/MIXED surfaced on session events (record 046).
- `SessionFileType` FILE, IMAGE, SLIDE, SLIDE_YAML, SLIDE_XML, WEBSITE, WEBDEV; `SessionFileStage` PROCESS vs PRODUCT; `SessionFileEditor` AI, USER_OWNER, USER_COLLABORATOR; `ArtifactSummary` origin and latest task (records 015, 016, 018).
- Session events (`session_event.proto`): title (with icon), credits, new message (unread counts, background activity), file change, canvas update, slides image update, webpage screenshot update, web-dev build status, app pack status (BUILDING, SIGNING, WAITING_2FA_CODE, WAITING_APPLE_CREDENTIALS, PUBLISHING, PUBLISHED), agent update, agent subtask update, team role change.

### 3.2 Projects [SRC, `project.proto`, `project_common.proto`]
`Project` has instruction (with version list and restore), team visibility (NONE/READ/WRITE), members with READ/WRITE/FULL_ACCESS, pending invitations, files and links, default connectors and skills, `shared_task_policy` (DISABLED, EDITORS, OWNER_ONLY), pinning and sort order. `ProjectUserCustomInfo` stores each member's personal overrides (instruction, mode, connectors, accounts, devices and folders, local permission mode, addon selection, "do not use my own skills"). `ProjectSkillConfig` can forbid member overrides (records 012, 013).

### 3.3 Automations [SRC and DOC]
Two families share one list (`AutomationListType` SCHEDULE, TRIGGER, ADVANCED_AUTOMATION):
- Scheduled tasks: type NO_REPEAT/DAILY/WEEKLY/MONTHLY/INTERVAL or cron, timezone, expire config, `run_as_new_task`, `run_mode`, tier, connectors with account refs, project, cloud computer, `target_device_id`, `target_folder_path`, playbook; creation response carries `max_count`, `current_count` and a count-limit rejection; the agent writes schedules itself through a sandbox API (record 049).
- Triggers: `TriggerConfiguration` with mode, instruction, target (current session or new session with task mode, cloud PC, desktop binding), expiry, a fixed set of entries (source, event type, filter, params, schedule spec, binding to mailbox, Slack channel or connector resource, webhook suffix) or a script (code plus schedule spec); revisions for optimistic concurrency; executions with stage, error code, materials; entry runtime status and `needs_attention`. The addon skill adds cadence minimums (10 min for events, 15 or 5 min for scripts by plan), dry-run limits (6 per minute), the rule never to poll from an agent loop, and the rule that payloads are untrusted (record 050).

### 3.4 Human-in-the-loop requests [SRC]
Typed requests and typed replies (record 127): confirm suggested action, resolve local execution approval, submit workspace directory access, confirm cascade workflow (with `estimatedAgentCalls`), confirm touchpoint action (call or SMS with formatted targets and an "always allow" config), respond to media-use confirmation, reply to a new-task suggestion, reply to API access approval for the tasks addon, confirm connector re-OAuth, confirm Meta ad account selection, reply to API credit notice, insufficient feature quota action.

### 3.5 Agents, computers and channels [SRC]
`Agent` carries id, name, email prefix and address, icon, `pc_id`, long-lived `session_uid`, `is_manus`, connectors, skills, phone number, instruction, payment ability, add-on overrides, pin and unread. Groups and channels hold several agents (and humans) with versioned close (`expected_version`). `AgentComputerPosition` records whether the agent is on its sandbox, a local desktop or a cloud computer, with authorization lists per computer (records 054, 055, 081).

### 3.6 Policy objects [SRC]
Data control levels with input and output levels per connector and tool and a session default (record 071); sandbox policy sets with allowlist or blocklist mode and an active pointer (073); connector team policies (072); observability destinations with protocol and tier (086); team roles and credit caps (074).

---

## 4. Feature areas, assessed

### 4.1 Wide Research (best-in-class in this audit)
Backend: the workflow composer addon (record 045) [DOC]. Frontend: a pre-run card with an estimated credit range and subtask count (Start or "Always auto-start" stored as `autoWideResearch`), a status grid of subtasks, a popover with live sub-session status, per-subtask retry and replay links, a mid-run card that reports credits already spent and asks Continue or Skip, a "wrap up now" instruction that asks the agent to present partial results, and plan-based concurrency and task limits (record 038). A confirm-workflow card gates large fan-outs by estimated agent calls (record 128).

Why it is good: cost is surfaced before the run and again when it matters; the user keeps control of a long job; failure is recoverable at subtask level; the engine is deterministic and replayable after restarts (journal of completed agent calls), has hard budgets (1 to 2000 calls with 10 percent headroom), pauses instead of failing at budget exhaustion, and forbids misuse (no fan-out for a deterministic script, one agent per item, no bundling).

Weak spots: the estimate is a range with no confidence information; mid-run gate triggers on credits, not on quality; the internal name `mapReduce` leaks into events and creation source; concurrency limits are plan-gated in ways that affect result completeness [INF].

Verevon today: `subagent.research_fan_out` exists (concurrency 4) with cards but no entry point, no cost gate, and cards are missing from durable replay (baseline gap 9).

### 4.2 Automations
Manus is ahead of a plain cron: unified list, calendar-like time axis, `Run now` and `Test run`, run history with unread markers and "Ask AI why it failed", an NL entry ("Describe when Manus should act ...") that produces a structured request to the agent, and event or webhook or script triggers with careful semantics (records 049-052). One interesting choice: the UI does not write automations directly; it sends the agent a message containing ids and `target_mode` lines (for example "Run target: reply in this same task (target_mode=current_session)") and instructs it not to run the action now. The agent is the single write path, so validation, authorization and audit live in one place [INF from strings plus the addon documents].

Weak spots: plan limits are tight (count limits, minimum intervals) and the UI mentions an "at most once per hour" limit for some automations; event polling cadences are catalog-driven rather than user-set; no evidence of run-level budgets per automation [INF].

### 4.3 Projects, library and sharing
Projects are real server objects with roles, instructions versions, shared files and links, team visibility, shared-task policy and per-member overrides. The Library is cross-task and aware of file stage (PROCESS vs PRODUCT) and edit provenance. Sharing combines private, team, public and collaborator modes with an admin request-and-approve step for public links, a "replay sharing" policy (including a strictest-wins rule for tasks that ran on a member's computer), and a rule that blocks collaboration for tasks that touched a phone, an email address or My Computer (records 008-018). These are enterprise-grade and well thought through. The unique move is treating replay visibility and local-file exposure as policy rather than as a side effect of a share link.

### 4.4 Creation studios
- Slides (record 022, 023) are engineered with the same discipline as the workflow engine: a tool-owned manifest, a per-slide lifecycle (`edit_pending`, `edited`), hooks on native file write and edit that sync, validate XML and measure HTML height, and a `present` gate that refuses delivery until every slide is authored and valid. This is the best deliverable pipeline in the audit [DOC].
- Video (records 024, 025): a re-editable timeline project that the agent and the human edit through the same schemas, fourteen model-visible tools, bundled ffmpeg, a WebGPU and WebCodecs cut renderer, and written workflows for UGC, long-to-short and slides-to-video. Desktop only.
- Web and app builds (records 030-034): websites, iOS and Android apps with build, signing and store submission states, desktop apps, browser extensions, widgets, bots, notebooks, games; hosting modes (Autoscale vs Reserved), domains, SEO, analytics, GitHub sync, sandboxed payments. This is a separate product bolted into the same chat; Verevon should not chase it.
- Phone (record 036): per-country numbers with included calls and SMS, per-use permission prompts, call history and recordings (up to 90 days), AI-voice disclosure text.

### 4.5 Connectors, skills, addons, widgets, tunnels
- Connectors (records 041, 042, 092): first-party set (Gmail, Outlook, Calendars, Notion, GitHub, Slack, Shopify, Instagram, Meta Ads, Google Ads, My Browser, Computer Use), per-task enablement and account selection, structured confirmation cards for writes, custom MCP over HTTP or SSE or stdio with OAuth detection and secrets, API connectors with `@name` secret references (shared vs per-user), publish to team or projects, enterprise registry.
- Skills (record 089): personal, team, project and hub scopes, version history and restore, import from GitHub with limits (100 files, 32 MB), per-skill apply policy, `Create skill with Manus`, and a skill recorder that learns from a demonstration (record 099).
- Addons (record 090): the manifest splits tools by caller (`modelVisibleTools`, `hostCallableTools`, `subjectTokenTools`), declares update mode, presentation flags, browser workers and a device service, and the host verifies archives by sha256. The widget host protocol (record 091) lets an addon render a sandboxed iframe that opens file bindings, saves with a revision check, reports view state and pushes selections into the composer as typed references.
- Tunnels (record 093): reverse tunnels for private MCP servers with per-route subdomains, scope-stackable publishing and per-route rate limits.

### 4.6 Settings, onboarding, i18n and appearance
- Settings are hash routes (`app#settings/<section>`): connectors, skills, usage, my-computer, remote-access, data-controls, browser, features, phone, deployments, scheduled-tasks, my-plugins, integrations, developers, access-tokens, and team sections (members, usage, skills, tunnels, access-controls, website-usage-billing) (record 103).
- Onboarding collects role, industry, company size and evaluation stage, offers team or personal paths, an invitation-code gate for private beta, verified-education access, memory import from another AI, recommended playbooks and suggested prompts, and an "Explore Manus 2.0" checklist stored as `explore_2_progress_v1` (record 106).
- i18n: English-keyed catalog with 24 locale bundles; rich-text placeholders; translator context embedded in keys (record 105). Appearance: accent presets, density, UI font size 12 to 16 px, corner radius (record 104).
- Accessibility signals exist but are partial (record 113). Keyboard shortcuts exist (mod+k, mod+shift+o, ctrl+n, mod+t and others) and a Shortcuts settings page; I found no dedicated command palette (low confidence).

### 4.6.1 Desktop shell
Floating always-on-top activity indicator with stop, mark-read, dismiss, open and expand; task windows with pin and drag preview; tray; a global Alt+M shortcut; updates deferred while tasks run; keep-awake (records 061, 110). Only relevant if Verevon builds a desktop shell.

---

## 5. What is best in class (ranked)

1. Wide Research end to end: pre-run and mid-run cost gates, per-subtask retry and replay, deterministic replayable engine (038, 045, 128).
2. Event-point replay with idle-gap compression and a single renderer for live, scrub and share (063, 009, 115).
3. Agent-suggested takeover with dual viewer, URL-addressable dialog and hand-back message (064).
4. Data classification as labelled information flow on sessions, connectors and tools (071).
5. Typed HITL request and reply taxonomy (127).
6. Scheduled-task and trigger data models, with explicit cadence, expiry, run target and revisioned updates (049, 050).
7. Slides manifest, lifecycle and present gate (022).
8. Browser contract: revision tokens, retry semantics, action ledger, staged sensitive input (067, 068).
9. Addon manifest with tool-visibility classes and a revision-checked widget protocol (090, 091).
10. Send control with explicit interrupt, steer and queue verbs (002).

## 6. Weaknesses and limits

- Feature sprawl: the product spans consumer growth mechanics (referral credits, trophies, events, arcade), many creation studios and a hosting business. The core agent loop is only one part of the surface [SRC].
- Credit complexity: five credit kinds with a documented consumption order (event, daily, monthly, add-on, free), per-tier multipliers, plan-gated concurrency, and special-case rules (free video, event credits). This is hard to explain and clearly caused many UI strings [SRC].
- Replay is only as faithful as stored events; sharing replays of local tasks needed an explicit policy because of leakage risk (010) [INF].
- English-string-as-key i18n with embedded translator comments is brittle (105) [INF].
- Several security-sensitive features are high-risk by design: browser credential import, agent payments, phone and SMS, agent mailboxes that create tasks from email (an injection vector unless sender allowlists are enforced), full-access local mode [INF].
- Desktop-only depth (video editor, computer use, skill recorder) means the web experience is a subset; evidence of equivalent depth in the browser version is thin.
- Memory depth: memories can be browsed and edited, personalization can be imported from another AI, and the agent proposes knowledge items for review; I found no evidence of an outcome-based learning loop or user model comparable to Hermes (the proposal loop is the closest) [INF].
- The in-app marketing claim of a transparent sandbox with no black box is only partly met: the panel shows actions, and reasoning is available as a Thinking stream, but I found no routing, token or tier-selection detail beyond mode names and credit counts [INF].

## 7. What Verevon should copy, adapt or avoid

Priorities consider Verevon's rules: Model Plane owns sessions, runs, sandboxes, browser grants and cost; the gateway may not store transcripts; Quarry executes browser actions the Model proposes; ZDR must propagate; Space access never widens an owner-plane ACL.

Copy (high value, fits the planes)
1. Wide fan-out UX with a pre-run estimate, start or adjust or skip, a mid-run spend gate and per-item retry. Owner: Model Plane (estimates from cost plane); UI in chat. Fixes baseline gap 9.
2. Steer, queue and interrupt as three explicit verbs with a remembered preference. Owner: Model Plane (already queues input) plus composer.
3. Timeline scrubber over stored run events with gap compression and "jump to live"; share it as a read-only replay projection. Owner: Model Plane (run events and /events/replay exist); replay must be ZDR-aware (no replay when ZDR forbids retention, or ephemeral replay with explicit expiry) [INF].
4. Agent-suggested takeover: the model emits a `suggest_takeover` proposal; Quarry grants human control and returns a hand-back note; credentials are typed by the human. Owner: Model Plane proposes, Quarry executes.
5. Typed HITL request and reply taxonomy using the same idempotent event shape Verevon already uses for approvals.
6. Server-owned Projects: instruction versions, roles, files, per-member overrides with an "enforced" flag. Owner: Session-core and Data Plane for files (no cross-plane DB access).
7. Automations as one list: schedules with expiry and run target, plus event and webhook triggers; use the agent as the single write path with revisioned updates and dry runs. Owner: Model Plane cron plus notification plane.
8. Browser tool contract improvements: revision tokens, retryable and requiresSnapshot error codes, action ledger and in-doubt state, host-bound secret references.
9. Data classification (levels on sessions, in and out levels on tools): maps to ZDR propagation and ACL rules and can be enforced in the capability policy.
10. Addon-style manifests that split tool visibility by caller (model, host, subject token) for Verevon's plugin runtime; revision-checked widgets for editable artifacts.

Adapt
- Library: a cross-thread outputs page built from Data Plane documents, not from model-plane tables.
- Agent mailbox: only with an allowlist and an "untrusted payload" rule (the trigger skill already uses it).
- Knowledge proposals: keep the propose, review, accept loop with a link to the originating event; store accepted items in Data Plane v2.
- Replay-sharing policy: make replay visibility a policy object with strictest-wins composition.

Avoid
- Consumer growth mechanics, five-kind credit systems and hosting or domain products.
- Importing browser passwords and cookies, and agent payments, until an owner-plane payment executor and audit exist.
- English-key i18n catalogs; keep stable ids.
- Letting the UI write automations directly if validation can instead be centralised in the agent tool.

## 8. Gap table: Manus versus Verevon (chat and agent pages)

| Area | Manus | Verevon today (baseline) | Action |
|---|---|---|---|
| Wide research | Cost gate, grid, retry, replay | Cards, no entry point, not in durable replay | Add entry and gate; persist cards in run events |
| Mid-run input | Interrupt, steer, queue verbs | Queued input, no verb choice | Add explicit verbs and preference |
| Replay and share | Scrubber plus share page plus policy | /events/replay exists; no share link | Build scrubber and read-only share projection with policy |
| Takeover | Agent-suggested, WebRTC and noVNC | Human/agent toggle in hosted browser; chat cannot drive browser | Add model proposal and hand-back |
| Projects | Server model with roles and versions | Browser-only organizer | Build in session-core |
| Automations | Schedules, triggers, history, NL entry | Cron with no-tool single call | Extend to run-as-new-task, expiry, triggers |
| Artifacts | Library, lineage, versions, conversions | Per-thread, read-only | Library page and edit-in-place later |
| Plan gate | submit_plan with file and stale protection | Editable plan with rungs | Add stale-plan guard and plan file download |
| Sensitive input | Temporary refs bound to hosts | Approval cards for risky actions | Adopt host-bound refs |
| Data controls | Labelled levels | ZDR and ACL rules | Express as levels in capability policy |
| Extensibility | Skills with scopes; addons; MCP from chat | MCP cannot run from chat | Prioritise MCP execution then manifest model |
| Mobile and desktop | iOS, Android, desktop shell | Responsive web | Out of scope short term |
| Sources and trace | Typed viewers per tool | Generic tool cards | Add typed viewers for search, terminal, editor |

## 9. REA usage log

REA tools actually used (stateless, path-based):
- `analyze_javascript_application` on `extract/manus/node_modules/@next-agent` (format directory, detail summary, integrity record-and-continue, 2 GB heap, 900 s timeout). Result: 213 relevant files, 209 JavaScript files parsed, 403,861 AST nodes, 1,171 findings, 355 limitations, coverage partial. Evidence id `ev_e0fb802f4ce520537ab63cdc91078c07b8b2e0bf1b7cfcb3e4b08884b19d7469`. The large result was saved to a file and queried with jq from stdin.
- `inspect_analysis_view` (modules page, offset 0, limit 40) on that evidence: listed BrowserMcpServer, BrowserDriver* modules, screen-live remoteInput and Rust-process modules.
- `trace_application_feature` (seed module contains `BrowserMcpServer`, direction both): 4 seed matches, 745 nodes, 900 edges, 369 terminal paths, 1,150 observed facts, 406 inferred facts, 346 unavailable facts. Evidence id `ev_e974b68e95c01957ca753b6048387eb3d85aa445971cbe645eacd8b703cb8752`.

Not used: `inspect_pe_resources`, `binary_overview`, `search_strings`, `list_strings` (no native document was opened, to avoid the shared REA active target), `analyze_web_bundle` (needs a live CDP page). The 124 MB renderer was analysed by direct reading and by decoding embedded descriptors rather than by REA, because one heavy analysis per app is the rule and the scout had already used it on `dist/`.

## 10. Open questions

1. How long and in what form does Manus retain event segments that drive replay, and how does replay interact with the data separation and deletion programme mentioned in the UI?
2. What exactly raises a session's data-classification level automatically (the strings say the product may raise it)?
3. Does the WebRTC viewer used for takeover also serve the passive live view, or is passive view derived from screenshots in events?
4. What is the actual cost model behind Wide Research estimates, and how is the range computed?
5. Which of the 144 services are internal or flag-gated and never reach the UI?
6. How are agent-proposed knowledge items ranked and merged, and is there deduplication across projects?
7. What masking, if any, applies to Amplitude autocapture in a product that handles customer data?
8. Is the plan-as-file model also used to persist agent plans across sessions or only for approval?

## 11. Evidence index

- English i18n dump: `extract/manus/frontend/out/_next/static/chunks/32_2-1jr2qqin.js` (keys are the English strings). Other locales: 23 sibling chunks with the same key set.
- Store and slices: `.../2jwv7cz4lh77_.js` offsets: agentWorkspace 309542, sessionSwitch 403060, timeline 450519, mapReduce 588019, meeting 588525, onboarding 720826, websocket 980168, sharePage 1007798, cascadeJobs 1074754, timelineUtils 762342-763500.
- Task UI: `.../0sydbzkqidpbv.js` offsets: planner dock 2055710, viewer switch 2039315, send control 2663493, confirm-workflow 3600527, touchpoint card 3610665, takeover card 4061548, plan card 5763031, Wide Research start card 5711499, subtask grid 5719800, spending card 6003798, loop card 6042970, error switch 6044332, Jump to live 6326374, WebRTC viewer 7696104, Exit takeover 7698792.
- Constants: `.../0ygo416gxr4ri.js` (connector ids at 164498, QUALITY_MODE, PAGE_LAYOUT_SIZE, PRODUCT_NAME_*).
- Chat socket: `.../0h7s-hxz9tj9m.js@751950`; custom MCP dialog and settings routes `@7313463`; widget host `.../1nlv87tzczom3.js`.
- Decoded protobuf index: reproduce by extracting `fileDesc("<base64>")` strings from the chunks and decoding the FileDescriptorProto wire format (scratch decoder: `tmp/mnux/pbdesc.js`, output `tmp/mnux/pbfiles.json`). Key files: `session/v1/session.proto` (SessionService, 83 methods), `session_base.proto`, `session_event.proto`, `project.proto`, `triggers/v1/triggers_base.proto`, `schedule/v1/schedule_sandbox.proto`, `agents/v1/*`, `enterprise/v1/enterprise.proto`, `team/v1/team_sandbox_policy.proto`, `connectors/v1/*`, `tunnel/v1/tunnel.proto`, `cloud_pc/v1/cloud_pc.proto`, `device_authorization/v1/device_authorization.proto`, `openapi/v2/openapi_v2.proto`, `telecom/v1/*`, `knowledge/v1/knowledge.proto`.
- Addon documents (extracted into scratch from `resources/managed-addons`): manus-tasks SKILL.md (28-80), manus-triggers SKILL.md (8-98), manus-workflow SKILL.md (10-132), manus-slides SKILL.md (22-43), manus-video-editor SKILL.md and addon.json, manus-storage (handoff) SKILL.md, manus-trip SKILL.md.
- TypeScript sources: `node_modules/@next-agent/browser-runtime/src/{browserTypes.ts, BrowserDiagnostics.ts, browserWorkspaceStore.ts, browserDriver/BrowserDriverPresentation.ts, browserDriver/base.ts, contract/browserContract.ts}`; `screen-live-service/src/{ScreenLiveTypes.ts, remoteInput/inputProtocol.ts}`.
- Desktop main process: `dist/{setupMenu.js, ipc/handlers/window.js, desktopActivityIndicator/*, windowManager/windows/DesktopActivityIndicatorWindow.js:49, keepAwake/keepAwakeService.js:66, autoUpdaterManager.js:42-51, agentManager/operatorManager.js:189-229,564, ipc/handlers/appearance.js}`.
