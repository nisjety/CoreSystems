# Slack: product-surface deep dive (audit 1, facet "product-surface")

Analyst: deep-dive (Sonnet). Source: Slack desktop 4.52.178 (Windows MSIX install, pre-extracted copy) plus public Slack docs. Companion file: `slack.product-surface.features.json` (155 records, ids `slack-product-surface-001..155`; `#NNN` below means that id). Scout inventory: `slack.inventory.md` (read, and the items I rely on were re-verified in the bundle).

Provenance tags: **REA** = observed through the REA analysis of the shipped artifact; **SRC** = read directly in the extracted files (I used small node scripts to read byte ranges of the minified bundles); **DOC** = Slack docs, help center, engineering blog, press (URL given in the JSON `evidence`); **INF** = my inference, basis stated.

---

## 0. What this source can and cannot tell us

1. **The Slack desktop app is an Electron shell around the remote slack.com web client.** The chat UI, the real-time client, huddles, canvases, lists, workflows, Slackbot and the admin console are not in the bundle. The bundle holds the shell: windows, tray, notifications, global shortcuts, deep links, update, OS integration, a typed native bridge and diagnostics (SRC `package.json`, `dist/main.bundle.cjs` 1.7 MB, `preload.bundle.js`, `component-preload-entry-point.bundle.js`). So the **product surface is mostly DOC**: 102 of 155 records are DOC, 49 are SRC, 3 REA, 1 INF.
2. **REA coverage was partial.** I reused the scout's retained analysis (`ev_05c9f0e1...`, directory analysis, record-and-continue) and the retained evidence was still readable. Facts REA gave: 454 parsed JS files, 1 contextBridge API, 4 sender-validation observations, 0 main-process `ipcMain.handle` handlers and only dynamic IPC channel expressions (0 literal channels). My own `trace_application_feature` call with the string seed `ADD_TO_SEARCH_INDEX` returned no match on a partial graph (`ev_cbbde6c1...`), although the string is present at `main.bundle.cjs@955861` (channel enum) and in the IPC schema table near `@998028`. REA's absence is therefore not evidence of absence; I recovered the channel names, settings schemas and handlers by reading the bundle (SRC). My `inspect_analysis_view` module-item call failed because many modules share the path `dist/main.bundle.cjs` (REA asked for a node_id).
3. **Docs were read with WebFetch (full page text through a summariser) wherever possible**; 11 evidence URLs are only known from web-search result summaries and are labelled "search-result summary only" in the JSON. Secondary sources (CNBC, No Jitter, Onna, Confluent) are low weight. Several Slack help-article IDs I guessed returned 404 (for example the Later/saved-items article), so those claims rest on other pages.
4. **No dynamic run.** Nothing was launched. Feature flags (for example `desktop_command_center`, `focus_sessions`) are visible by name; their default values are unknown.
5. **Secret-bearing stores were not read** (toolbelt tokens file, auth-cookie backup, any userData content). Only code and the shipped locale files were read.

Offsets such as `main.bundle.cjs@995451` are byte offsets in the extracted minified bundle (paths are relative to the scratchpad root). Source maps are not shipped.

---

## 1. Executive summary

**What Slack is, in one line:** a channel-centred message fabric (human + app + agent as the same kind of participant), with a very strong platform (events, Block Kit, MCP, Real-time Search) and enterprise controls (Grid, Connect, EKM, retention, audit, discovery), and a 2026 pivot where Slackbot becomes a personal agent and orchestrator for third-party agents.

**Best-in-class (relative to what a Verevon Space needs):**
- Cross-organisation rooms: Slack Connect (#021, #022, #131).
- The agent-in-the-room *contract*: agent surfaces, session status, streaming plan/task cards, app-context, identity and permission rules, HITL guidance (#080-#087), plus a governed MCP server (#088, #089) and a permission-bounded read path for agents (#045, #085).
- Attention management: activity inbox, layered notification preferences, cross-device routing, focus/DND awareness and a notification health engine in the shell (#049-#059).
- Enterprise governance: layered retention, customer-held keys, data residency, audit and discovery APIs (#119-#125).
- Real-time scale: documented channel/gateway/presence fabric and cell-based resilience (#107-#110).

**Table stakes revealed by Slack (everything a Teams/Slack-class Space must have):** channels + DMs + group DMs, threads, reactions, edit/delete, mentions with group handles, files with previews, status/presence, search with AI answers, notifications with schedule/mute/keyword/VIP, an app/bot platform with slash commands and interactive blocks, workflow automation, retention + audit + SSO/SCIM, a desktop shell with tray, deep links, auto-update, i18n and accessibility.

**Differentiators (Slack-specific, hard to copy):** Slack Connect; Marketplace gating of data APIs (MCP and Real-time Search only for directory or internal apps); Slackbot as personal agent plus orchestrator (vendor claims, partly upcoming); Salesforce-native data in the room (#076, #094).

**What Verevon's Space lacks vs Slack (from the Verevon baseline, DOC `findings/t1/v-space-backend.baseline.md`, `v-space-ui.baseline.md`):** a human-to-human message path, DMs, reactions, edit/delete, attachments, threads panel, push realtime (it polls every 6 s), notifications from Space events, mute/DND, a personal inbox, canvases/lists, calls, a personal agent, external identities, and any SCIM. **What Verevon has that Slack does not:** signed, effect-bound Control decisions with receipts, deny-by-default per-org effect policies, governed agent members (Control roster plus Application binding), a durable agent workspace (sandbox lease), Quarry browser grants, ZDR propagation intent, and a deletion saga with per-plane receipts. Slack leaves agent compute, browser and PC sessions to third parties (#098).

---

## 2. Architecture

### 2.1 Client shell (SRC)

- Electron app; `package.json`: `slack-desktop` 4.52.178, main `dist/boot.bundle.cjs`, Node >= 24, Electron 44.4.5 as devDependency, rspack/swc build, Electron Forge makers (DMG, ZIP, Squirrel, MSIX, WiX MSI) (#133).
- **Thin by design.** The web app is loaded remotely; the shell injects an inlined `windowchrome` script with `webFrame.executeJavaScript` (`preload.bundle.js@762108`), exposes `window.slackPlatform` (P@762422) and, for component windows, `window.desktop` (store dispatch, diagnostics, intl) (C@618065) (#113).
- **State:** a Redux store in main with redux-observable epics, mirrored to preload and renderer over one IPC channel named `redux-action` (M@222740, P@451557, C@440620); `slack-persistor` / `root-state.json` persists desktop state (M@832096) (#112, REA/SRC).
- **Bridge:** 122 IPC channels are declared in one Joi schema table (`main.bundle.cjs@995451`). Invalid calls are rejected and reported to `/beacon/ssbipc` (M@994739) (#146). The web app can push 32 whitelisted settings with range checks (M@1108892) (#147).
- **Hardened windows:** frozen webPreferences (nodeIntegration false incl. workers and subframes, contextIsolation true, sandbox true, webviewTag false) with an override guard that logs an error (M@969388, @969582) (#127).
- **Layered configuration:** settings have `name`, `external` name, schema, platforms and scope (policy or default); macOS via cf-prefs, Linux via `/etc/slack-desktop.conf`; keys include DefaultSignInTeam, HardwareAcceleration, DownloadPath, WebSwitches, AutoUpdate, ClientEnvironment, ReleaseChannel, SignInMethod (M@830467, @838114, @838867) (#027).

### 2.2 Real-time fabric (DOC, Slack Engineering "Real-time Messaging", 2023)

- Client opens one WebSocket to the nearest edge; Envoy terminates TLS; a **Gateway Server** (stateful, in-memory, multi-region) holds user connections and subscriptions.
- A **Channel Server** (stateful, in-memory) owns a hashed subset of channel IDs (user, team, file, huddle and channel IDs are all hashed); **consistent-hash ring managers (CHARMs)** with Consul publish the ring; **Admin Servers** are stateless bridges from the web app backend; **Presence Servers** are hash-partitioned and queried through the gateway only for users visible on screen.
- Message path: client -> web API -> Admin Server -> owning Channel Server -> all subscribed Gateway Servers -> clients. About 500 ms global delivery; a failed Channel Server is replaced in under 20 s; a failed region is drained to the nearest healthy one. Transient events (typing) skip the database. Top-of-hour spikes (reminders, scheduled messages, calendar events) are an explicit load input (#107, #108, #015, #111).

### 2.3 Data and resilience (DOC)

- MySQL + **Vitess**; sharding moved from workspace-keyed to **channel-keyed** because Grid and Connect broke "one workspace per shard"; about 2.3M queries/s peak, p99 about 11 ms at the time of that post; CDC to Kafka through the Debezium Vitess connector (#109).
- **Cell-based architecture**: AZ-siloed backends, Envoy with an in-house xDS control plane (Rotor), weighted-cluster drains in about 5 minutes with 1% granularity, born from a 2021 gray-failure outage (#110).

### 2.4 Platform and agent protocols (DOC)

- **Events API** over HTTP (3 s ack, 3 retries at 0/1/5 min, 30,000 deliveries per workspace per app per 60 min, auto-disable above 95% failures) and **Socket Mode** (outbound WebSocket opened with `apps.connections.open`, envelope_id acks, up to 10 connections) (#066, #067).
- **Web API** rate-limit tiers (about 1/20/50/100 per minute plus special) and 1 message/s/channel pacing (#074).
- **MCP server** at `mcp.slack.com/mcp` (Streamable HTTP, JSON-RPC 2.0, confidential OAuth, per-tool user-token scopes, directory or internal apps only, admin approval, audit logs) (#088, #089, #090).
- **Real-time Search API** (`assistant.search.context`): user token or per-event action token, semantic mode triggered by question-shaped queries on Slack AI Search plans, no storing/copying/training on results, guests excluded (#045).
- **Agent session API**: `agents.sessions.setStatus` processing/active (stuck for one hour if not cleared), `chat.startStream/appendStream/stopStream`, `task_update` and `plan` display modes, `app_context_changed` with an ordered entity list, `agent_session_stopped` (#081-#083).

---

## 3. Data model (what the evidence supports)

| Object | Shape / identity | Notes and evidence |
|---|---|---|
| Conversation | channel, private channel, DM, MPDM, huddle; all addressed by ID and hashed to Channel Servers | #001, #107 |
| Message / thread | message timestamp; thread by parent `thread_ts`; agent threads reuse `thread_ts` | #002, #081 |
| Event envelope | `type, team_id, api_app_id, event, event_id, event_context, event_time, authorizations, is_ext_shared_channel, context_enterprise_id` | #066 |
| Block | JSON blocks array; 50 per message, 100 per modal or Home; `action_id`/`value` interactions | #069 |
| App identity | registered app with fixed app ID; bot token, user token, app-level token (`xapp`), action token | #067, #045, #089 |
| File | upload URL + file_id, malware scan, shared to channels; remote files with tombstones | #008 |
| Canvas / List | document or structured items attached to a conversation as a tab or standalone; owner, view/edit grants | #100-#102 |
| User / group | SCIM-synced users, guests, user groups (workspace or org level) | #024 |
| Org | workspace(s) under an Enterprise Grid org; Connect relationships between orgs | #018, #021 |
| Notification | `notificationId, channel, teamId, messageId, userId, threadTimestamp, traceId`; toast launch args carry the same triple | #058 (SRC) |
| Desktop settings | 32 validated keys; policy/default/user layers | #147, #027 (SRC) |

---

## 4. Shell findings worth knowing (SRC; each has a record)

1. **Presence from OS idle time** (#014): every 10 s `powerMonitor.getSystemIdleTime() <= 20` means ACTIVE; when active and not locked it sends `TICKLE_MESSAGE_SERVER` to every loaded web-app window (M@1326611, @1326957).
2. **Notification stack** (#055-#059, #063): native toast (winrt) with workspace header grouping, circle-cropped avatar, reply box and actions; HTML popup fallback with corner/display/max-items/timeout; startup reset of a stored failover; notification-warnings engine with reasons QUIET_HOURS, Windows notifier states, GPU composition unavailable, zero max/timeout, unknown method, recent failover (cache 30 s with warnings, 60 min without), macOS permission re-request capped at 3 per session and 10 minutes apart (M@1040390, @1044800-1048200, @1346463-1349800).
3. **Update restart veto** (#037): `QUERY_STEALTH_RESTART_SAFE` -> `STEALTH_RESTART_SAFE_RESPONSE {requestId, safe, reason: huddle|upload|guard}` (M@996535).
4. **Deployment tiers** (#125): NORMAL / RESTRICTED (GovSlack) / MILITARY with separate hosts, log directories, Sentry proxy, cookie blocking and a per-mode feature table; mode recomputed at sign-out (M@1326100-1326611, @834036, @791677, @1071229).
5. **Screen-share border window** (#033), **virtual cameras / audio device probes** (#035), **media-access utility process** (#036), **global mute and command-center hotkeys with key-down/key-up combo registration** (#135, #093).
6. **OS search index and local file search** (#048): `ADD_TO_SEARCH_INDEX`, `REMOVE_FROM_SEARCH_INDEX`, `SEARCH_LOCAL_FILES`, `GET_RECENT_FILES`, `UPLOAD_RECENT_FILE` (M@998028, @998859).
7. **Three-state network status** (#114): `/api/api.test` probe distinguishes OFFLINE from SLACK_DOWN (P@654500-656200).
8. **Cookie rewriting** (#116): Set-Cookie for selected hosts is re-issued with SameSite=None and a `safeStorage`-encrypted value (M@863689). Purpose is INF.
9. **Command center / "Slackbot Mini"** (#093): tray click and Ctrl/Cmd+Shift+K send `desktop_command_center` to the web app; the name "Slackbot Mini" is only a log string (M@1210075). Not documented publicly (a web search found nothing), so treat as flag-gated.
10. **Launch integrity check** (#128): on child/renderer process loss, compares the app bundle modification date with its launch value (M@1175562).

Not in the shell (so not evidence either way): reactions, threads, polls, drafts, scheduled send, huddle UI, canvases, lists, workflows, admin console, SCIM. Treat the locale strings and the 122 IPC names as "native hooks", not as proof of product features.

---

## 5. Notable design decisions (and why they matter to Verevon)

1. **One conversation primitive for humans, apps and agents.** Agents arrive through the same message events (`message.im`, `app_mention`) and threads (`thread_ts`), and are controlled through app scopes. This keeps humans able to see and join agent work. Verevon already models agents as room members; the gap is the human side of the primitive (#001).
2. **Permission follows the requesting user**, not the agent: Real-time Search is a user-token or event-bound action-token call, scope per content type, guests excluded, no retention or training. Verevon's signed per-effect Control decisions are stronger and should keep ZDR-style "no copy" terms for provider calls (#045, #085).
3. **Directory as trust boundary.** The sharpest APIs (MCP, RTS) are limited to directory-listed or internal apps; non-Marketplace commercial apps get stricter history limits (#065, #089). Verevon has no app marketplace; a registered, admin-approved client identity per connector is the equivalent control.
4. **Agent UX is a first-party container**, not each vendor's chat widget: split view, session status, streaming with plan/task cards, suggested prompts, feedback buttons, app-context entity list (#080-#083). That makes every agent feel the same and gives Slack a place to enforce disclosure rules.
5. **Design guidance over protocol for HITL.** Slack tells developers to preview and require explicit approval for irreversible actions and to start restrictive, but ships no approval protocol; approvals are app-level (#087). Verevon has an approval proxy but the granted approval does not resume the paused run on the Space path (baseline); fixing that beats Slack's state.
6. **Attention is configurable by source.** Activity always contains mentions, reactions, thread replies; DMs, "all new posts" channels, reminders and suggested important unreads are opt-in; mobile pushes wait until the desktop is idle (#052, #053).
7. **Separate durable and transient paths** (typing, presence) and **subscribe presence only for visible users** (#015, #108).
8. **Sidebar is user data.** Custom sections are per-user, with filter/sort per section, and are even covered by EKM (#023).
9. **Hard environment tiers** in the client (commercial / GovSlack / military) rather than a server-side flag (#125).
10. **Messages as the system voice:** retention approvals and pin removals arrive as Slackbot DMs (#062).
11. **Scale and resilience as documented design**: consistent hashing, in-memory channel layer, AZ drains (#107-#110).
12. **Product churn is acceptable on the surface, not in the model:** channel canvases were retired for canvas-as-a-tab (conversion from 9 April 2025) without breaking the underlying document concept (#100).

---

## 6. Agents in the room: Slack's model vs the Verevon target

| Topic | Slack (DOC unless noted) | Verevon today (baseline) | Take-away |
|---|---|---|---|
| Agent identity | App/bot users with function-based names and non-human avatar; labels for on-behalf-of and unreviewed AI (#084) | Control service subject + Convex binding; Agent chip | Add on-behalf-of and "AI, unreviewed" labels |
| Personal agent | Slackbot, private per user, permission-bounded (GA Jan 2026; Business+/Enterprise+) (#077) | Absent: personal Space cannot hold an agent | Highest-value gap |
| Invoking | DM, split view, optional @mention; app-context entities (#080, #083) | `@agent` mention, one agent per message | Add context entities and a side panel |
| Progress visibility | `processing` status, plan/task cards, recap with links (#081, #082, #086) | 6 s poll of thread status, "is working" row | Adopt task/plan states and run recap |
| Approvals / HITL | Guidance: preview, explicit approve, restrictive defaults, review surface (#087) | Approval cards, owner-bound; grant not resumed on this path | Finish resume; add preview and review surface |
| Multi-agent | Slackbot routes to third-party agents; MCP client upcoming (#078) | No agent-to-agent delegation in the room | Treat Slack's version as direction, not proof |
| Tools / MCP | Slack MCP server with admin approval and audit (#088, #089) | MCP registry not Space-scoped | Space-scoped MCP surface with approved client IDs |
| Memory | None on the platform; app state-object guidance (#091) | No `space` memory scope | Differentiator if Verevon ships Space memory |
| Compute / browser / PC | None first-party (#098) | Sandbox lease, Quarry-governed browser | Verevon differentiator |
| Cost / limits | Plan-gated AI features; Slackbot reported as not separately metered (secondary source) (#097) | Org budget hard stop only | Add per-Space allowance and plan gating |
| Scheduled agent work | Workflow schedule trigger, summarise-channel step (#096) | Space-scoped cron with Control fire decision | Surface authoring inside the room |
| Feedback | feedback buttons, reactions (#095) | none | Add thumbs plus reaction events |
| Summon | Tray/Ctrl+Shift+K command center, flag-gated (#093) | none | Add global summon to the desktop client |

---

## 7. Gap matrix against Verevon (by taxonomy category)

Verevon status comes from the two baseline files; "Adopt" = copy Slack's behaviour; "Adapt" = copy the idea, change the mechanism to fit planes/authority; "Avoid" = Slack choice that conflicts with Verevon rules.

| Category | Verevon lacks (per baseline) | Adopt / adapt | Avoid |
|---|---|---|---|
| Messaging core | Human-only posts, DMs, reactions, edit/delete, attachments, unfurls, drafts/schedule, @person records, saved items | Thread model and follow rules (#002), reactions as events (#005), two-step file upload (#008), scheduled send (#010), reactions-as-feedback | Per-message edit history without a retention policy (see #006) |
| Org & membership | External/guest identity, team groups, role change, room discovery, SCIM | Connect owner-org model (#021), role ladder (#019), custom sidebar sections (#023), SCIM (#024) | Slack's workspace-as-data-boundary; use Space id as the shard key (see #109) |
| Calls & meetings | Everything | Huddle-as-attachment with thread and canvas (#031), AI notes to canvas (#032), screen-share border (#033), update veto (#037) | Treating calendar/whiteboard as native (#040) |
| Search & discovery | Message/thread search, directory, AI answers over Space content | Modifier grammar (#041), RTS-style per-request permission (#045), Space-scoped MCP search | Unbounded cross-workspace search scope; keep Space-level ACL |
| Notifications & attention | Space-event notifications, mute/DND, inbox, cross-room unread | Activity inbox (#053), cross-device routing (#052), notification health (#056), toast reply (#057) | Email-first approvals |
| Apps & extensibility | Bots, webhooks, app directory, slash apps | Events envelope (#066), Socket Mode (#067), Block Kit-like contract (#069), App Home as agent review surface (#070) | Non-namespaced slash commands (#068) |
| Agents in the room | Personal agents, multi-agent, side panel, task/plan states, feedback | See section 6 | Guidance-only HITL (#087); unlisted agents locked out of MCP |
| Collaborative artifacts | Canvas, lists/boards, room files | Canvas-as-tab (#100), Lists with views and item threads (#102), document grants | Auto-archiving the oldest list items at a cap (#102) without notice |
| Realtime & sync | Push transport (polling every 6 s), offline cache, multi-device | Gateway/channel fabric concept (#107), visible-user presence (#108), reconnect/network states (#114) | Per-request SSE only for the author's turn |
| Governance, security & compliance | Space audit view, DLP, export, legal hold, EKM, residency | Retention layers (#119), audit schema (#120), EKM and residency (#122, #123), compliance targets (#124) | Metadata outside customer keys without disclosure (see #122) |
| Client shell | Desktop shell | Thin shell over the web UI (#133), deep links (#134), validated bridge (#146), managed settings (#027), staged update veto (#037) | Hiding internal developer menus in production builds (#138) |
| UX patterns & layout | Cross-room rail, thread panel, command palette | Left rail (#148), header shelf (#149), thread panel (#150), quick switcher (#153) | Burying the agent entry in a flag-gated hotkey (#093) |

---

## 8. Weaknesses and limits (what Slack does not do well, or what we cannot see)

- **Agent runtime is out of scope for Slack** (no sandbox, browser or PC); multi-agent orchestration is announced direction with no mechanism detail (#098, #078).
- **HITL is guidance, not protocol**; there is no platform approval object (#087).
- **No E2EE**; confidentiality relies on TLS, at-rest encryption and optional customer keys, and profiles, channel names and membership stay under Slack keys (#118, #122).
- **Free-plan limits and plan gating** shape the UX: 2-person huddles, one canvas per conversation, AI by plan (#031, #100, #097).
- **Slash commands are not namespaced** (latest install wins) and do not work in threads or the split view (#068).
- **MCP and RTS are closed to unlisted apps**, which blocks bespoke internal-tool agents unless they are internal apps (#089).
- **Product churn:** channel canvases retired; the 2026 Slackbot relaunch overlaps the older Slack AI (#100, #077).
- **Vendor claims:** Slackbot capability and "no extra inference cost" come from press and marketing; the MCP client in Slackbot is labelled upcoming (#077, #078, #097).
- **Shell-level oddities:** the macOS notification-state helper appears stubbed; internal developer menu items (TSAuth, local assets, tracing UI) ship in production menus; fuse hardening is INF (#059, #138, #127).

---

## 9. What Verevon should copy, adapt or avoid (prioritised)

**Copy (high leverage, low conflict with Verevon rules):**
1. Human message path with reactions, edit/delete and attachments, using the existing Space/Control authority; add the two-step upload and unfurl contract (#001, #005, #006, #008).
2. A personal Activity inbox (mentions, approvals-needed, agent-finished, assigned items) and cross-device routing based on desktop presence (#052, #053).
3. Agent UX contract: session status (`processing`/`active` with a stuck timeout), plan/task cards, run recap with links, feedback buttons, app-context entities (#081-#083, #086, #095).
4. Identity and disclosure rules for agent output (on-behalf-of, unreviewed AI) and "private info stays ephemeral" (#084, #085).
5. A thread side panel, command palette (rooms, threads, agents, skills) and left-rail navigation (#148-#153).
6. Modifier search plus an MCP/RTS-style read surface scoped to a Space (#041, #045, #088).
7. Update veto handshake: let long agent runs and pending approvals block a restart (#037).

**Adapt (copy the idea, change the mechanism):**
1. Slack Connect -> Verevon S5.1: external identity with owner-org control, per-org custody and labelled sidebar group, expressed through Control memberships (#021, #131).
2. Real-time fabric -> keep Convex as system of record but add a thin push channel keyed by Space id with visible-member presence (#107, #108).
3. Retention, audit, discovery -> Space-scoped retention policy and a Space audit stream with actor/action/entity/context/details and an `app`/agent field (#119-#121).
4. EKM and residency -> tenant keys on the Data Plane and region as a Space/tenant attribute (#122, #123).
5. Deployment tiers -> a ZDR/sovereign client profile (endpoints, logs, telemetry, cookies) (#125).
6. Directory-as-trust -> admin-approved connector identities for MCP/webhooks (#065, #089).

**Avoid:**
1. Workspace-as-data-boundary and workspace-keyed sharding (it broke Grid and Connect) (#109).
2. Non-namespaced slash commands and skills (#068).
3. Guidance-only approvals; Verevon's receipts and decisions are better, finish the resume path (#087).
4. Silent auto-archival at list caps (#102).
5. Metadata exemptions from customer keys without clear disclosure (#122).

---

## 10. Open questions

1. What exactly does the web client do with `TICKLE_MESSAGE_SERVER` and with the OS idle state (presence only, or also push suppression)? (#014)
2. What is `desktop_command_center`/"Slackbot Mini" in the UI, and is it generally available? (#093)
3. How are polls, forwarding/sharing of messages and drafts implemented? I did not verify them (no fetched source), so they are not in the records.
4. How does Slack's AI inference path handle retention (ZDR-like) and sub-processors? The data-residency page does not mention AI (#123, #126).
5. Which objects are pushed to the OS search index (#048), and what do `canvasHandoffEpic` and `spotlightHandoff` do (only names found)?
6. What is the real mechanism behind Slackbot orchestration of third-party agents, and when does the Slackbot MCP client ship? (#078)
7. Is "Slack Atlas" a Slack product? A web search found Atlas only as Salesforce's reasoning engine in Agentforce (2025); I did not record it as a Slack feature.
8. How do Slack's cell migration and Vitess channel-sharding interact with Grid and Connect today (the posts are 2020-2023)? (#109, #110)

---

## 11. Evidence appendix

**Local evidence (SRC/REA)** - `extract/slack/package.json`; `extract/slack/dist/main.bundle.cjs`, `preload.bundle.js`, `component-preload-entry-point.bundle.js`; `dist/resources/locales/en/desktop_{browser,external,general,menu,messagebox,renderer}_en_GB.json`; `dist/notifications.{html,bundle.js}`, `settings-editor.bundle.js`, `net-log-window.bundle.js`, `basic-auth-view.bundle.js`; `tmp/slack_const_names.txt`, `tmp/slack_event_codes.txt`. Key offsets (main bundle): IPC schema table 995451; shortcut map 995140; Slackbot/command-center tray 1210075; jump list 1209064; idle presence 1326611/1326957; toast builder 1346463-1349800; notification warnings 1040390-1048200; per-mode feature table 1071229; mode switching 1326100-1326611; managed settings 830467/838114/838867; frozen webPreferences 969388; integrity check 1175562; validated settings 1108892; cookie rewrite 863689. Preload: network status 654500-656200; windowchrome 762108.

**REA tool log (this run)** - `inspect_analysis_view` (summary view and an ambiguous module-item call) and `trace_application_feature` (string seed `ADD_TO_SEARCH_INDEX`, no match) against retained parent `ev_05c9f0e1dfaf0c7a27858a5136a7f42bd98da55e8da0938a470ad14b7bfe2552` (scout's single `analyze_javascript_application` run, not repeated). `search_strings`, `list_strings`, `inspect_artifact` and PE tools were not applicable (no native binary analysed).

**Docs read in full via WebFetch** - features page; docs.slack.dev AI overview, MCP server, MCP harnesses, agent design, developing AI apps, agent context management, Real-time Search API, Block Kit, slash commands, Socket Mode, Events API, rate limits, App Home, files, workflows, audit logs API; help articles for huddles, clips, canvas, lists, notifications, Workflow Builder, Slack Connect guide, retention, EKM, search, AI guide, roles, sidebar sections, status, threads, Discovery API, data residency, SCIM, formatting, keyboard shortcuts; compliance page; Slackbot press release; two Slack blog posts; three Slack Engineering posts.

**Search-result summaries only** - pins and bookmarks, Connect approvals, admin-approved invitations, Grid workspace management, canvas sharing and channel-canvas retirement, scheduled-send guides, No Jitter Slackbot article, Onna Discovery/legal-hold article, Confluent Debezium session.
