# Microsoft Teams (new client, MSIX 26260.1701.5139.3736): product-surface deep dive

Audit 1 (Verevon "Space"), facet "product-surface". Companion file: `teams.product-surface.features.json` (167 records, ids `teams-product-surface-001` to `-167`; this document cites them as "rec NNN"). Read-only audit, 2026-10-13. Nothing was launched or modified; the only files written are under the scratchpad.

## 0. Scope, method, evidence key and honest limits

**What this is.** A product-feature map of Microsoft Teams built from (a) the shipped Windows package, mostly its localized string tables, serve configs, manifest and JS chunks (tags REA or SRC), (b) Microsoft Learn and developer-blog pages fetched in this run (tag DOC), and (c) a few explicit inferences (INF). Teams is a thin shell around a web client delivered from Microsoft servers, so the package shows the shell, the native media stack, the endpoint allow-list and, above all, a complete en-US UI vocabulary for roughly 41,700 strings. Server-side behaviour (chat service, compliance stores, Copilot) comes only from public docs.

**Evidence aliases used below.**

- `WA` = `C:/Program Files/WindowsApps/MSTeams_26260.1701.5139.3736_x64__8wekyb3d8bbwe` (read-only MSIX).
- `HA` = `WA/desktop-assets/hashed-assets` (6,918 files, of which 6,902 are `.js.gz` chunks).
- `LOC` = `HA/main-locale-en-us-23ba9acb36586a4b.js.gz`. I decompressed a copy into the scratchpad and parsed it into 16,397 key/value pairs in 49 namespaces (activity, auth, autosuggest, channels, chatList, communities, compose, configurationAgent, contacts, extensibility, files, framework, messaging, nova, presence, settings, shortcut(s), simpleCollab and others).
- `FEAT` = 48 per-feature en-US locale chunks in `HA` (meeting-recap, meeting-collaboration, calling, teams-and-channels, custom-copilot, search, people-targeting, premium-benefits, semantic-object and more). Parsed: 25,288 pairs.
- Protocol chunks: `HA/247813-...` (content types and ID prefixes), `HA/30208-...` (Substrate search, object gateway), `HA/114419-...` (8.8 MB conversation and push code), `HA/190135-...`, `HA/143297-...`, `HA/213029-...`.
- `SERVE` = `WA/desktop-assets/serve-configs/work.json` (71,952 bytes; CSP and headers). `MF` = `WA/AppxManifest.xml`. `DJ` = `WA/locale/generated-touchdown/en-us/desktop.json` (88 keys).
- DOC pages fetched and read in full: Learn "teams-channels-overview", "limits-specifications-teams", "retention-policies-teams" (Purview), "security-compliance-overview", "facilitator-teams", "set-up-channel-agent-teams", "teams-end-to-end-encryption", "policy-assignment-overview", "communicate-with-users-from-other-organizations", Graph "teams-concept-overview", agents-in-teams overview, the archived bots overview, and the developer blog "Build collaborative agents where work happens". Seen only as search-result summaries (lower confidence, tagged medium): connector-retirement blog, Message Center mirrors MC1182703 and MC1243546, the "Building Agents for Teams" blog, the Teams developer announcements page.

**REA usage log (what was really run).**

| REA tool | Target | Result |
|---|---|---|
| `analyze_javascript_application` (directory, summary, record-and-continue, 2 GB heap) | decompressed main en-US locale chunk | ev `ev_39d25f183a1785432ef6d573ff47624f610dc6049c890c98ab568a0c9941aa15`: 1 file, 51,048 AST nodes, 49 webpack modules, 0 parse failures, application coverage complete |
| same | 7 protocol-bearing chunks (10.3 MB) | ev `ev_9541a00bd1baec69e1da291e4432bc7c6cc7b59405e3a8c8b307bd39c8287461`: 2,036,616 AST nodes, 394 modules, application coverage partial (semantic graph budget hit on 5 files) |
| same | 48 per-feature locale chunks | ev `ev_e84811b1636e07bac0ced49b0aab7a4c14c369bf50b08ce272b838258887ec72`: 48 files, 55 modules, 0 parse failures |
| `inspect_analysis_view` | page of modules; one module item | confirmed webpack runtime `webpackChunk_msteams_react_web_client` and module identities |
| `trace_application_feature` (string seeds) | four seeds: a consent key, a consent sentence, `/v1/users/ME/conversations`, `go.trouter.teams.microsoft.com` | all returned no match: REA's graph does not index string-literal object values or URL substrings in these bundles, which is not evidence of absence |

Not used: `open_binary`, `binary_session`, `search_strings`, `list_strings`, `binary_overview`, `inspect_pe_resources`, `inspect_artifact`, `analyze_web_bundle` (the scout had already run `inspect_pe_resources` and a partial analysis of `client-bindings`).

**What the REA tag means in the JSON.** The string text itself was extracted by my own tokenizer from files that REA parsed successfully; REA contributed module structure, parse validation and identity, not the string lookup. Treat REA as "shipped-artifact string-table evidence, structure confirmed by REA". Records read straight from JSON, manifest or binary strings are tagged SRC.

**Verification done on the scout inventory.** I re-read the manifest (protocols, capabilities, share target, startup task, COM), `action_manifest.json`, `configuration_gcc.json`, the `desktop-assets` package, `trusted-ui` and `client-bindings` (bridge factory and Trouter symbols present), the 63 locale folders and `locale/fallback-map.json`, and the real names of data-layer chunks. Corrections to the inventory: scheduled send exists (it is behind right-click Send; the scout found no `send_later` key); the locale fallback map sits at `WA/locale/fallback-map.json`, not under generated-touchdown; `maglev-configs/*.json` are tiny (6 keys) while the CSP lives in `serve-configs/*.json`; polls are not a native message type (only a search string).

**Limits of this document.** Static analysis only. Per-feature rollout state is unknown (strings ship before features). Server logic, ranking, storage schemas and Copilot behaviour are invisible. "Uniqueness" is judged against the reference set named in the audit brief, but Slack, Grok Bot, buzz and qm were not re-checked in this slice, so treat `uniqueness` as a first estimate (INF). Quality scores are relative maturity judgements, with reasons in each record's notes.

## 1. Verdict in one paragraph

Teams is, I expect (INF, the other references were not re-checked here), the most governance-complete team chat product in the reference set and, since 2025-2026, a serious agent host. Its chat core is conventional (threads or posts, reactions, mentions, tags, scheduled send, forward and move) and its differentiators are elsewhere: an enterprise administration model (policies, Purview retention, eDiscovery, DLP, labels, information barriers), a very developed attention system (activity feed with 61 typed event kinds, inbox "slices", follow rules, urgent breakthrough), and, most relevant for Verevon, a set of agent patterns that put authority on the screen: review-then-share answers, who-approves consent cards, per-conversation knowledge lists with delete-on-removal, scheduled status reports with a human review gate, and per-meeting AI access switches including a privacy mode that blocks all agents. What Teams does not show is the thing Verevon already has pieces of: an agent with its own computer, browser or sandbox, per-agent memory, or per-room cost. Its agent platform is also still mostly preview and carries many documented limits (one agent per channel, no private channels, no external users, no Customer Key).

## 2. Architecture: what runs where

| Layer | What it is | Evidence |
|---|---|---|
| Package and shell | MSIX with four apps: `MSTeams` (ms-teams.exe, full trust), `MSTeams.Update`, `msteamsautostarter`, and `MSTeamsRemoteModuleContainer` (partial trust, AppContainer). WebView2 runtime minimum 128.0.2739.67. | MF; rec 142, 136 |
| Web client ("Maglev") | `@msteams/maglev-included-web-assets` 260813193.14.50801226 plus `@msteams/trusted-ui` 260825087.0.50565228; 7,014 asset-manifest entries; root URI `https://teams.microsoft.com/v2`, statics from `teams.public.onecdn.static.microsoft`; `localIndexSupported: true`. React, Fluent UI, Apollo and GraphQL, Fluid, Yjs, Adaptive Cards (library identification from scout marker scan, INF medium). | `WA/desktop-assets/package.json`, `maglev-configs/work.json`; rec 142, 119 |
| Data layer | Apollo-style GraphQL client with 187 `data-resolvers-*` chunks, one per domain (chat, channel-messages, reply-chains, mention, notifications-engine, policies, tenant-membership, unified-consent, user-activity-recall, live-state-broadcast, meeting-summary-qna-history, browser-augloop-proxy, floodgate...). | file names in `HA`; rec 119 |
| Push transport | Trouter WebSockets (`*.trouter.teams.microsoft.com`, GCC and dogfood variants); native SlimCore also hosts a Trouter client with request, response, message-loss and user-activity-state calls; chat REST under `/v1/users/ME/conversations`. | SERVE; `client-bindings/scwv2_bundle.js`; chunk 114419; rec 117, 118 |
| Search and signals | Substrate (`wss://substrate.office.com`, `SignalServiceForSubstrate`), query alteration, message-id anchored results. | chunk 30208; rec 120, 053 |
| Co-authoring | Augloop (`*.augloop.svc.cloud.microsoft`) and Fluid with an ODSP driver; Loop hosts. | SERVE; chunks 143297, 191142; rec 121, 112 |
| Meetings | `*.attend.teams.*` (attendee service), `*.cts.speech.microsoft.com` (captions), `*.enterprisevoice.svc.cloud.microsoft`, `biometricsvc.teams.cloud.microsoft`, `*.ecdn.teams.*` and `*.peer5.skype.net` (enterprise and P2P CDN), `pptservicescast` (PowerPoint Live). | SERVE; rec 044, 045, 046 |
| Media | Native SlimCore (RTP/SRTP, ICE and SDP, DTLS, DX11 video, WASAPI) behind a WebView2 host bridge (`window.pieSharedBridgeFactory`, `chrome.webview`); Rust DTLS core (`dimpl 0.7.2`, 39 string hits); VDI bridges. | `client-bindings/*`, `ic3rustcore.dll`; rec 122, 150 |
| Hardening | Enforced CSP with `require-trusted-types-for 'script'` and named trusted-types policies, nonce scripts, wss-only connect list (44 entries), Document-Isolation-Policy, COOP, Reporting-Endpoints, per-worker CSPs. | SERVE; rec 135, 164 |
| Clouds | Separate serve configs for work, life (consumer), gcch, dod, gallatin, bleu, delos, ag08, ag09. | `serve-configs/`; rec 124 |

Two observations worth keeping. First, almost everything that matters is a separate service the client reaches over WebSocket or HTTPS (push, search, co-authoring, meetings, speech, storage); the chat service is just one of them. Second, the client treats credentials, account switching and OS pickers as a separate "trusted" bundle, which is a deliberate trust boundary around the code that renders other people's and agents' content (rec 136).

## 3. Data model and identity

- **Typed conversation IDs.** Group chats and meeting chats end in `@thread.v2` (meeting chats start `19:meeting_`), 1:1 chats in `@unq.gbl.spaces`, legacy in `@thread.skype`, system threads are `48:notes`, `48:notifications`, `48:calllogs`, `48:generic`. Member IDs are prefixed: `8:` users (`8:orgid:`, `8:acs`, `8:gcch-acs`), `28:` bots, `4:` PSTN, `50:sms:`, `sfb:`. A `threadType` flag, `uniquerosterthread` and `gapDetectionEnabled` accompany them (rec 001; chunks 247813, 30208, 213029). Note the word "spaces": the ID suffix and a `SkypeSpaces_ConversationPost_Extension_AmsReferences` message-extension property (chunk 30208) show that "space" is an inherited internal term for a conversation container in Teams (INF, medium), which is a naming coincidence Verevon should keep in mind when comparing vocabulary.
- **Message content types.** `Text`, `RichText`, `RichText/Html`, `Media_Card`, `Media_FlikMsg`, `UriObject`, `Contacts`, `Files`, `Sms`, `Location`, `Event/Call`, `Signal/Flamingo`, plus `Control/Typing`; `importance` as a property; images via an object gateway with named renditions (rec 002).
- **Hierarchy.** Team (backed by a Microsoft 365 group) contains channels of three types; chats are separate containers; communities are another container; meetings own chats. Limits: 25,000 members per team, 1,000 channels per team, 5,000 members in private and shared channels, 250 people in a chat (rec 020, 021, 125).
- **Storage split (DOC).** Chat and channel messages live in an Azure-powered chat service as primary storage; compliance copies go to hidden mailbox folders (user mailboxes for chats, group mailboxes for channel messages, SubstrateGroup for shared channels); files in SharePoint (each channel is a folder; private and shared channels get their own site) and OneDrive; recordings in OneDrive or SharePoint; Loop pages and Facilitator or Channel Agent artifacts as `.loop` files in OneDrive or the channel folder (rec 128, 111, 087, 092).
- **Policy and entitlement.** Every feature area has typed policies (messaging, meetings, calling, apps, encryption); one effective policy per type via direct assignment, then ranked group assignment, then the global default (rec 034). Licences gate AI features and the client explains refusals (rec 037, 098).
- **Graph.** Everything in the UI is also reachable through Microsoft Graph (teams, channels, chats, messages, tabs, installed apps, calls and online meetings), and the admin centre and PowerShell are built on it (rec 078).

## 4. Protocols and realtime behaviour

1. Web and native share one push system: Trouter. The native stack exposes `TrouterCreate`, `TrouterListenerCreate`, `OnTrouterConnected`, `OnTrouterDisconnected`, `OnTrouterRequest`, `TrouterSendResponse`, `OnTrouterMessageLoss` and `TrouterSetUserActivityState` to the web layer through the host bridge (rec 117). Message-loss events pair with `gapDetectionEnabled` on conversations (rec 118).
2. Degraded mode is designed, not accidental: when the sync service disconnects the UI tells the user it is loading updates and that sending and replying still work; threads panes say they are offline and ask to reconnect (rec 118, 123).
3. Search is a separate backend with its own query alteration and result anchoring (`;messageid=`), not a feature of the chat service (rec 053, 120).
4. Co-authored artifacts use their own real-time transport and storage ACL, which is why Loop components survive being pasted anywhere and fail with explicit access errors when the recipient lacks rights (rec 112, 121).
5. Scale policy is explicit: typing, read receipts and calling switch off above 20 chat members; real-time updates in shared channels reach only 25,000 users; mentions of team and channel are blocked above 10,000 members (rec 009, 023, 125).
6. Encryption: in transit and at rest, optional tenant-level Customer Key, and true E2EE only for 1:1 calls and meetings (with a 20-digit security code and a long list of features that turn off, including Copilot, recording and breakout rooms). Chat and channels are not E2EE (rec 047, 126).

## 5. Product surface by taxonomy category

### 5.1 Messaging core (rec 001-019, 159-161)
Conventional core with a few real differentiators. Best: system messages as typed timeline entries for every state change (rec 160, q5), presence with status message shown on mention and auto work location (rec 017), mentions including tags and agents (rec 018), group-chat history-sharing choice at join time (rec 016), per-channel Threads versus Posts layout (rec 003), priority messages with repeat notification (rec 005), moving a chat's recent messages into a channel (rec 012) and chat-to-channel conversion (rec 158). Weak spots: polls are an app (rec 019), scheduled send is hidden (rec 006), and edit or delete is governed by flags rather than a visible audit twin in the UI (rec 007).

### 5.2 Org and membership (rec 020-037, 158)
Teams, channels, three channel types with a feature matrix that has many exceptions (bots, Planner, moderation and scheduled meetings vary by type; rec 021), shared channels across tenants with trust cues (rec 023), a newer flexible channel model where everyone in the team can view, react and reply before joining (rec 022), tags (rec 025), guest versus external access versus B2B direct connect (rec 026), and a policy-assignment admin model (rec 034). User-owned sidebar sections with sort modes (rec 028) and inactive-channel clean-up suggestions (rec 029) are best-in-class for noise control.

### 5.3 Calls and meetings (rec 038-052, 162)
Largest surface by string count (the calling locale alone has 8,496 keys) and mostly out of scope for a text-first room. Valuable ideas: meeting recap with citations to the recording and permission-aware sharing (rec 042, q5), explicit per-meeting AI access and privacy mode (rec 043, q5), AI Interpreter in two modes (rec 044), content bin showing who lacks access (rec 162), and a live-meeting indicator in the channel header (rec 040).

### 5.4 Search and discovery (rec 053-058)
Typed operators `from:`, `in:`, `with:`, `is:`, per-conversation and per-person scopes with shortcuts, domain tabs, AI answers with citations, and select-text-then-Ask-Copilot with prebuilt prompts (rec 056).

### 5.5 Notifications and attention (rec 059-068)
The strongest category. The activity feed has 61 typed titles; the three-pane view has slices for Mentions, Threads, Tag mentions, VIP, Reminders, Drafts, Saved and Requests with per-slice show rules (rec 060); thread-follow rules (mentioned, authored, participated; rec 061); notification settings per channel, chat, sound, position and email (rec 062); DND that lets urgent and priority contacts through (rec 063); AI catch-up summaries (rec 066). Chat and spam requests expire after 28 days (rec 132).

### 5.6 Apps and extensibility (rec 069-081, 163)
Capability model for apps (tabs, bots, message extensions, connectors, meeting apps, personal apps, agents, MCP tools; rec 069), resource-specific consent with scope-named permission sentences (rec 071), Adaptive Cards, slash commands, Workflows (Power Automate) replacing Office 365 connectors in May 2026 (rec 076), Teams SDK with MCP and A2A (rec 077) and a public Graph API (rec 078). The one-line summary: Microsoft packages agent tools as app capabilities and asks the user for permission at use time.

### 5.7 Agents in the room (rec 082-110, 167)
See section 6.

### 5.8 Collaborative artifacts (rec 111-116)
Files delegate to SharePoint and OneDrive; Loop components and pages are live artifacts embedded in messages (rec 112); meeting notes are co-edited by everyone and by the agent (rec 113); lists, Planner, wiki, whiteboard and forms are tabs (rec 114, 115). The pattern to note is "message holds a pointer, artifact has its own ACL and store".

### 5.9 Realtime and sync (rec 117-127, 164)
Section 4 above.

### 5.10 Governance, security and compliance (rec 128-141)
Purview retention with hidden compliance stores and staged deletion (rec 128), the full compliance suite (rec 129), inline DLP with explain, report and override (rec 130), sensitivity labels with documented inheritance gaps for AI output (rec 131, 141), report and block flows (rec 132), app and agent governance (rec 133), Trusted Types and strict CSP in the shipped client (rec 135), a trusted-UI split (rec 136), sovereign clouds and AI data-boundary caveats (rec 137, 140).

### 5.11 Client shell (rec 142-150, 165)
Thin full-trust MSIX shell hosting WebView2, tray and jump list with presence statuses, autostart, updater with rings and an ECS flag editor (rec 144), error curtain and renderer watchdog with a "Trace" option and a close prompt that protects an unsent message (rec 145), 63 locales with a fallback map and high-contrast icon sets (rec 148), custom shortcut assignment (rec 147), preview programs (rec 149), VDI bridges (rec 150).

### 5.12 UX patterns and layout (rec 151-157, 166)
Unified three-pane list with a reversible switch-back (rec 151, q5), inline thread replies inside the inbox (rec 152), pinned-app bar with spoken drag-and-drop (rec 153), purposeful zero states (rec 154), narrated list rows (rec 155, q5), list-reshaping settings with system-generated sections that users can disable (rec 156).

## 6. Agents in the room: the part that matters most for Verevon

### 6.1 The agent types Teams ships or previews

| Agent | Scope and visibility | Evidence |
|---|---|---|
| Copilot in chat and channels | Private to the asking user; summaries, decisions, action items, time windows; side panel with Copilot and AI notes tabs; credits in consumer flavour | rec 089, 099 |
| Facilitator (meetings) | Public participant in the meeting chat, notes tab co-edited by everyone, only licensed users can initiate, GA subset plus Planner preview | rec 087 (DOC) |
| Group-chat AI notes | Enabling it makes the agent join the chat for everyone; visible to licensed members with full history | rec 088 |
| Channel Agent ("project agent" in client strings) | One per standard or shared channel, created by default unless the admin disables it, public preview | rec 082, 092-094 (DOC plus REA) |
| Team Agent | Plugin bundles (skills, tools, automations) for one team; "Under construction" | rec 096 |
| Configuration agent | Changes user settings from natural language through a closed set of typed actions | rec 100 |
| Intelligent call delegation | Personal agent that answers the user's calls under written instruction templates | rec 101 |
| Agent Assist | Pane for call-centre staff with customer search and sourced answers | rec 104 |
| Third-party agents | Built with Teams SDK (JavaScript, C#, Python), registered by API key, OAuth or Entra SSO, distributed through the agent store; partners named in the Build 2026 blog (GitHub Copilot, Atlassian Rovo, Cursor, Linear) | rec 077 |

### 6.2 Interaction models
Teams distinguishes private (one user, one agent), public (visible to the conversation) and the new targeted message (a private message between one user and an agent inside a group conversation, per the Build 2026 blog; GA date 2026-07-30 from a search summary). A private agent conversation can be escalated into the group chat, and the dialog says that interactions then become visible to everyone (rec 086). Agents are invoked by @mention in channels, group chats and meetings, by slash command, and can react to emoji and feedback signals (rec 107). Compose-time AI (rewrite, suggested replies, "ask Channel Agent" inserting an @mention) stays in the composer until the user sends (rec 090, 091).

### 6.3 Three consent patterns worth copying
1. **Review-then-share.** The agent answers using only what the requester may open; when other members cannot, the response is a private draft with the warning that it might contain information not everyone may access, plus Allow to send it to the group, an expiry, and a hard block "only you can see it" for sensitivity-restricted content (rec 083, DOC and REA). This is the clearest UX expression of Verevon's rule that Space access never widens an owner-plane ACL.
2. **Who approves.** In shared chats the agent either asks a named person ("I'll need your consent") or the room ("Can someone approve?"); the outcome is a line in the thread ("You approved this request", "{user} denied the request"), and previews offer Allow, Edit or Reject (rec 084).
3. **Tool access in the conversation.** "To complete this request, I'll need access to this tool": Add and Allow, Allow, Deny, with MCP tool name and description rendered, and plain failure messages that tell the user to resend after installation or sign-in problems (rec 085). Resubmission rather than resume is a hint that the pending request is not durable, the same limitation Verevon documents for approvals.

### 6.4 Knowledge, memory and scheduled work
- Knowledge is an explicit list of conversations (channel, selected meetings and chats), visible to everyone with access to the agent; meeting summaries wait 24 hours for approval before joining the knowledge base; removing the agent deletes what it learned; re-adding starts from zero; recurring series bring at most one prior transcript and 28 days of chat (rec 093; Learn). This is a complete answer to "what does the agent remember about this room and who can change that", with no free-form memory.
- Scheduled work exists as status reports (one time, daily, weekly, monthly) with named reviewers, an approve and share step, and a system message in the chat whenever anyone changes the schedule, prompt, reviewers or keywords (rec 092).
- No free-form agent memory, no per-person context and no tool-call card UI were found in the strings (rec 110, INF). Reports and answers are artifacts (`.loop`) or messages.

### 6.5 Controls, limits and cost
Admin allow or block per agent app, app policies by group, auto-creation toggle, web-search switch (which also disables web search for all Copilot features), and four distinct refusal reasons shown as tooltips (channel setting, feature flag, not a member, unlicensed) (rec 098, 133, 140). Limits: one Channel Agent per channel, private channels and external users unsupported, Customer Key tenants unsupported, previews may process data outside EU boundaries (rec 082, 137). Cost surfaces as monthly credits (consumer), promotional @mention limits and "subject to capacity" language; there is no per-room cost view (rec 099).

### 6.5b What is absent
No agent computer, browser or sandbox surface and no computer-use control in 41,700 strings (rec 109); the only code-execution hint is a "Code interpreter" skill on the agent profile page. This is where Verevon is ahead on paper: a Space workspace with sandbox leases and a background-process registry exist in its backend and browser actions are Quarry-governed (not yet Space-bound), but none of it is visible in the room yet (Verevon baselines).

## 7. Where Teams is best-in-class (candidates for "best of all")

| Area | Teams pattern | Records |
|---|---|---|
| Agent answer authorisation UX | review-then-share, expiry, sensitivity block, partial-history warnings | 083, 084 |
| Agent knowledge governance | explicit conversation list, 24 h approval, delete on removal, zero-state re-add | 093 |
| Per-room AI access control | AI toggle, privacy mode, ephemeral answers, who can manage AI | 043 |
| Attention management | typed activity feed, inbox slices with show rules, follow rules, DND breakthrough | 059-063 |
| Timeline as audit | system messages for every state change | 160 |
| Sidebar organisation and safe IA migration | user sections, sort modes, switch-back modal with data preserved | 028, 151 |
| Permission consent text | scope-named permission strings per chat, team or user | 071 |
| Governance depth | policy precedence with rollout log, retention, DLP, labels, IB | 034, 128-132 |
| Accessibility | narrated rows, spoken drag and drop | 153, 155 |
| Client hardening | Trusted Types plus strict CSP, trusted-UI split | 135, 136 |
| Scale policy | published limits with defined degradation | 125 |

## 8. Weaknesses and limitations of Teams (as observed)

1. **Channel-type matrix.** Standard, private and shared channels differ in bots, Planner, moderation, scheduled meetings, analytics and agents; users get "some apps will be removed" warnings (rec 021, 022). Verevon should avoid creating room types that fragment capabilities.
2. **Agent platform immaturity.** Channel Agent is public preview with one-per-channel, no private channels, no external users, no Customer Key; Team Agent is "under construction"; naming churn (Teams AI library to Teams SDK, Toolkit renames); doc conflicts on language support and GA state (rec 082, 096, 077).
3. **AI compliance gaps admitted in docs.** AI notes not auto-collected in eDiscovery, no retention labels on cloud attachments in AI notes, sensitivity labels not inherited by AI output, watermark and copy restrictions not applied to Facilitator answers (rec 087, 131, 141).
4. **Hidden-state compliance.** What the UI shows is not what is retained; deletion chains can take weeks (rec 128). Users cannot see their own retention state.
5. **E2EE and AI are mutually exclusive.** Enabling E2EE disables Copilot, captions, recording and breakout rooms (rec 047).
6. **Discoverability and flag sprawl.** Scheduled send is behind right-click; many features are licence or admin gated, so the same button is absent for different reasons; experiment-arm labels leak into strings (rec 006, 037, 151).
7. **No visible agent execution.** No tool-call timeline, no cost view, no agent workspace; consent flows rely on resubmission (rec 085, 099, 109).
8. **Doc and source conflicts to carry forward.** Facilitator docs say unsupported in group chats while the client ships group-chat AI notes under the same branding (rec 088); the Teams SDK Python status differs between the archived bots page and the Build 2026 blog (rec 077); one tracker lists Channel Agent as GA while Microsoft's own page says preview (rec 082).

## 9. What Verevon should copy, adapt or avoid

Verevon facts come from `v-space-backend.baseline.md` and `v-space-ui.baseline.md` (read this run); every Teams statement is tagged in the records.

### 9.1 Table-stakes gaps that Teams fills (needed regardless of agents)

| Verevon baseline gap | Teams reference | Records |
|---|---|---|
| Every post is a model turn; no human-only message | typed content types and system messages; composer-side AI stays unsent until the user posts | 002, 090, 160 |
| No DMs, group DMs, edit or delete, reactions, attachments, message search, mentions record | typed conversation IDs, edit/delete under policy, reactions, files via owner plane, operators and scoped search, mention records feeding the feed | 001, 007, 008, 111, 053, 054, 018 |
| No notifications from Space events; unread is room-level and for the open room only | typed activity feed, inbox slices, follow rules, DND, mark-all-read | 059-063, 067 |
| 6-second polling, no push | one push channel with request/response and message-loss signals, explicit degraded mode | 117, 118 |
| Personal Space cannot hold an agent; no personal agents | private agent threads, escalate to group, personal call-delegation agent with guardrail templates | 086, 101 |
| Non-owners cannot reply; approvals are owner-bound and granted approvals are not resumed | any-member or named-approver consent cards with audit lines; Edit before Allow; note that Teams also resubmits | 084, 085 |
| No guests or external; no teams or sub-groups | Team layer, tags, guest tiers, shared channels with trust cues and expiring invites | 020, 023, 025, 026 |
| No room discovery or join | flexible channels with preview before join, discover feed | 022, 058 |

### 9.2 Patterns that fit Verevon's authority-first design (copy first)
1. **Review-then-share answers** for any agent output grounded in owner-plane data (rec 083). It uses Control decisions Verevon already mints and turns them into one visible step.
2. **Per-conversation knowledge lists** with delete-on-removal and re-add-from-zero as the first Space memory scope (rec 093, 110).
3. **Per-room AI access switch and privacy mode** as a room setting that overrides binding policy, with ephemeral versus retained answers mapped to ZDR (rec 043).
4. **System messages for governance events** (member added, agent bound, paused, revoked, policy changed, schedule edited) written into the room timeline in addition to the Activity tab (rec 160, 092).
5. **Scope-named permission strings** at agent-add time generated from Control scope names (rec 071).
6. **Typed event catalogue** for the feed and for slices that hold approvals waiting, failed runs, agent mentions and followed threads (rec 059-061).
7. **Status reports as scheduled agent work with a reviewer gate** instead of Settings-only cron creation (rec 092).
8. **Message-level "Ask agent" actions** with prebuilt prompts (rec 056), and an in-composer rewrite that does not create a room post until sent (rec 090).
9. **Trusted Types plus pinned connect-src and isolation headers** for the SPA and, if a desktop shell follows, a separate trusted bundle for credential and OS surfaces (rec 135, 136).
10. **Reversible navigation experiments** with a switch-back that preserves user data (rec 151).

### 9.3 Adapt carefully
- Channel agent per room: Verevon's many-agent binding model is richer; copy only the default-on, admin-toggle lifecycle (rec 082).
- Team layer: introduce Team as the owner of many room Spaces and default roster before adding channel-type variants (rec 020, 021).
- MCP as an app capability with per-use consent: align with Verevon's MCP registration, scoped to a room, with Control owner-grant decisions (rec 069, 085, 095).
- Retention: copy staged deletion and hold-over-delete precedence, not the hidden-store opacity (rec 128).

### 9.4 Avoid
- Capability matrices per room type (rec 021).
- Single-agent-per-room simplifications and licence-gated invisibility (rec 082, 037).
- Resubmit-instead-of-resume consent flows (rec 085).
- Leaving AI output outside content-protection and label policy (rec 131, 141).
- Per-feature preview badges without a visible state of what is on for this user (rec 149).

## 10. Open questions (for the synthesis step)
1. Which of Slack, Grok Bot, buzz and qm already match the review-then-share and per-conversation knowledge patterns (rec 083, 093)? If none do, they are differentiators to carry.
2. Does Grok Bot expose agent PC and browser state in the conversation (rec 109)? That is the missing half of the hybrid.
3. How does the Channel Agent actually evaluate "everyone in the conversation can open this file" at response time (rec 083)? Only the behaviour is documented.
4. What is the durable object behind "agent sessions" (rec 107)? The client strings show nothing.
5. Rollout state of flexible channels, Team Agent, HTML widgets and the LLM-judge dashboard (rec 022, 096, 103, 108) cannot be read from a static package.
6. `odsl_*` (180 keys) and `nova`, `esp`, `brb`, `sht`, `tmp_` prefixes are unexpanded; meanings are inferred from usage only.
7. The Teams SDK "agent sessions" and targeted-messages dates rest on search summaries; re-read the primary pages before quoting dates.

## 11. Appendix: record index by category
(Generated from the JSON; id, quality, uniqueness, provenance, name.)

**Messaging core**

- 001 | q4 | uncommon | REA | Conversation types and typed identity namespace
- 002 | q4 | uncommon | REA | Message content-type model (text, rich HTML, cards, files, location, call events)
- 003 | q4 | uncommon | REA | Channel layout choice: Threads vs Posts
- 004 | q3 | uncommon | REA | Cross-post one message to several channels
- 005 | q4 | uncommon | REA | Priority messages with repeat notification (Important and Urgent)
- 006 | q3 | common | REA | Scheduled send
- 007 | q4 | common | DOC | Edit, delete and tombstones under messaging policy
- 008 | q4 | common | REA | Reactions, GIFs, stickers, memes and org-wide custom emoji
- 009 | q4 | common | DOC | Read receipts and typing with participant cut-offs
- 010 | q4 | common | REA | Pinned posts, pinned chats and saved messages
- 011 | q3 | common | REA | Multi-select forward with validations
- 012 | q3 | uncommon | REA | Move messages from a group chat into a channel
- 013 | q3 | common | REA | Audio and video clips in chat
- 014 | q4 | common | REA | Rich-text composer with code, tables, Loop paragraph and configurable Enter
- 015 | q4 | common | REA | On-demand translation of received messages
- 016 | q4 | uncommon | REA | Group chat membership with history-sharing choice and join policy
- 017 | q5 | common | REA | Presence, status message and auto work location
- 018 | q5 | common | REA | Mentions: people, channel, team, tags and agents
- 019 | q2 | common | REA | Polls posted as messages
- 159 | q4 | common | REA | Quoted replies with verification state
- 160 | q5 | common | REA | System messages as first-class timeline entries
- 161 | q3 | uncommon | REA | SMS and Phone Link interop chats with explicit feature limits

**Org & membership**

- 020 | q5 | common | DOC | Team and channel hierarchy with owner, member and moderator roles
- 021 | q4 | uncommon | DOC | Three channel types: standard, private and shared
- 022 | q4 | unique | REA | Next-generation flexible channels: preview before joining and per-user shown channels
- 023 | q4 | unique | DOC | Shared channels across teams and tenants with trust cues
- 024 | q4 | common | REA | Channel moderation and reply restrictions
- 025 | q4 | common | REA | Tags: @mentionable groups of people
- 026 | q4 | common | DOC | Guest access, external access and B2B direct connect compared
- 027 | q4 | uncommon | REA | External-chat consent and extended directory
- 028 | q5 | uncommon | REA | Left rail with Favorites, Muted and custom sections
- 029 | q4 | unique | REA | Inactive channel clean-up suggestions
- 030 | q4 | common | REA | Team creation from templates, existing groups, classes and clones
- 031 | q4 | common | REA | Channel and chat info pane
- 032 | q3 | common | DOC | Channel email address with sender-domain allow-list
- 033 | q3 | uncommon | REA | Communities (Viva Engage-backed) with join approval and admin checklist
- 034 | q5 | common | DOC | Policy-based administration: global default, group and user assignments, packages
- 035 | q4 | common | DOC | Archive, delete and 30-day channel restore
- 036 | q4 | common | REA | Multi-tenant identity: tenant picker, cross-tenant redemption and multiple accounts
- 037 | q4 | uncommon | REA | Licence-gated features with in-product upsell and request-a-license
- 158 | q4 | unique | REA | Convert a group chat into a channel with history

**Calls & meetings**

- 038 | q4 | common | REA | 1:1 and group calling from chat with ring-others and call limits
- 039 | q5 | unique | REA | Telephony: call queues, call park, delegation, hot-desking and PSTN
- 040 | q5 | common | REA | Meetings: Meet now, scheduled meetings and channel meetings with a live indicator
- 041 | q4 | common | REA | Meeting recording and transcription with storage lifecycle
- 042 | q5 | common | REA | Meeting recap: AI notes, tasks, topics, chapters, speaker timeline, share
- 043 | q5 | unique | REA | Copilot and AI in meetings with privacy mode and ephemeral answers
- 044 | q4 | uncommon | REA | AI Interpreter and live captions in many languages
- 045 | q4 | common | REA | Breakout rooms, whiteboard, Together mode and share tray
- 046 | q4 | unique | REA | Webinars, town halls and events with production studio and AI highlights
- 047 | q4 | uncommon | DOC | End-to-end encryption for 1:1 calls and meetings with security code
- 048 | q4 | common | REA | Meeting options, roles, lobby, green room and admin-locked settings
- 049 | q4 | uncommon | REA | Agenda tracker, timers and meeting goal detection
- 050 | q4 | common | SRC | Calendar, scheduling and Outlook integration
- 051 | q4 | common | REA | Call health, quality feedback and voice-isolation profile
- 052 | q3 | uncommon | SRC | Devices, rooms and desk booking
- 162 | q4 | uncommon | REA | Meeting artifacts: attendance report, content bin, analytics and speaker coach

**Search & discovery**

- 053 | q5 | common | REA | Unified search with typed operators (from, in, with, is) and query alteration
- 054 | q5 | common | REA | Scoped search: this channel, chat, thread, post or person
- 055 | q4 | common | REA | Result domains: messages, files by type, images, meetings, people, channels, settings
- 056 | q4 | uncommon | REA | AI answers and citations inside search and Copilot overlay on selected text
- 057 | q4 | common | REA | People directory, external lookup and contact management
- 058 | q3 | uncommon | REA | Discover feed and suggested chats

**Notifications & attention**

- 059 | q5 | common | REA | Activity feed with 60 typed event kinds and filters
- 060 | q5 | unique | REA | Inbox slices: Mentions, Threads, Tag mentions, VIP, Reminders, Drafts, Saved, Requests
- 061 | q4 | common | REA | Thread following with explicit rules (mentioned, authored, participated)
- 062 | q5 | common | REA | Granular notification settings per channel, chat, sound, position and email
- 063 | q4 | common | REA | Do not disturb, priority contacts and urgent breakthrough
- 064 | q3 | common | DOC | Missed-activity email digests
- 065 | q4 | uncommon | REA | Reminders on chats and channels, shared with everyone in the chat
- 066 | q4 | uncommon | REA | AI catch-up: Holistic Recap and Summarize what I missed
- 067 | q3 | common | REA | Mark all as read across chats, channels and communities
- 068 | q4 | common | SRC | Urgent-signal escalation through native toasts, taskbar badge and sounds

**Apps & extensibility**

- 069 | q5 | uncommon | REA | App capability model: tabs, bots, message extensions, connectors, meeting apps, personal apps, agents, MCP tools
- 070 | q4 | common | REA | App store and catalogue with ratings, pricing models, certification and install stats
- 071 | q5 | uncommon | REA | Resource-specific consent: scope-named permission prompts per chat, team or user
- 072 | q4 | common | DOC | Bots: personal, group chat and channel scopes with activity handlers and blocking
- 073 | q4 | uncommon | REA | Adaptive Cards with charts, dynamic search and file consent
- 074 | q4 | common | REA | Message extensions and input extensions (link unfurling)
- 075 | q4 | common | REA | Slash commands and command autosuggest
- 076 | q4 | common | DOC | Workflows (Power Automate) templates and incoming webhooks replacing O365 connectors
- 077 | q4 | common | DOC | Developer platform: Teams SDK, 365 Agents SDK, Agents Toolkit, Copilot Studio with MCP and A2A
- 078 | q5 | common | DOC | Microsoft Graph for Teams: teams, channels, chats, messages, tabs, installed apps, calls and meetings
- 079 | q3 | uncommon | SRC | Shell integration points: protocol handlers, share target and Windows AI actions
- 080 | q3 | uncommon | REA | Choose-a-place app starts (contextless configuration) and embedded chat
- 081 | q4 | common | REA | Third-party tabs and pinned apps in chats and channels
- 163 | q3 | common | REA | Share to Teams from other surfaces with account-mismatch handling

**Agents in the room**

- 082 | q3 | unique | DOC | Channel Agent: one dedicated agent per channel (identity and lifecycle)
- 083 | q5 | unique | DOC | Permission-aware answers with review-then-share to the group
- 084 | q4 | unique | REA | Group-chat human-in-the-loop consent: any member or a named requester approves
- 085 | q4 | unique | REA | Inline tool-access consent with Add and Allow, including MCP tool descriptions
- 086 | q4 | unique | DOC | Private versus group agent interactions, targeted messages and escalation to the group
- 087 | q4 | unique | DOC | Facilitator: AI meeting facilitator visible to all participants
- 088 | q3 | uncommon | REA | Group chat AI notes by enabling the facilitator agent in a chat
- 089 | q4 | common | REA | Copilot in chats and channels: summaries, decisions, action items and time ranges
- 090 | q4 | common | REA | AI rewrite in the composer: tone, length, proofread and custom instructions
- 091 | q4 | uncommon | REA | Suggested replies and suggested actions (create task, remind me, schedule meeting, ask Channel Agent)
- 092 | q4 | unique | REA | Status reports as scheduled agent work with reviewers and a 24-hour knowledge opt-out
- 093 | q5 | unique | REA | Per-conversation knowledge scoping with approve-or-expire meeting summaries and clean removal
- 094 | q4 | uncommon | REA | Channel Agent configuration: keywords, model choice, language, proactive help and scheduling
- 095 | q3 | uncommon | REA | MCP connectors for room agents and MCP tools as app capability
- 096 | q2 | unique | REA | Team Agent: plugin bundles that add tools, skills and automations to a team-scoped agent
- 097 | q4 | uncommon | REA | Agent profile page: permissions, skills, data security, pricing and AI labelling
- 098 | q4 | common | REA | Admin and licence gating for agents
- 099 | q3 | uncommon | REA | Usage credits, promotional limits and capacity caveats
- 100 | q4 | unique | REA | Configuration agent: change settings by describing the intent
- 101 | q4 | unique | REA | Intelligent call delegation: a personal agent that answers your calls under written guardrails
- 102 | q4 | uncommon | REA | Plugin manager for Copilot with per-plugin toggles and data-sharing disclosure
- 103 | q2 | uncommon | REA | Generated HTML widgets rendered in messages
- 104 | q3 | uncommon | REA | Agent Assist for call-centre agents
- 105 | q3 | uncommon | REA | Agent-driven task management: assignments, due dates, goals and Planner write-back
- 106 | q4 | common | REA | AI labelling, citations and feedback on every agent surface
- 107 | q3 | uncommon | DOC | Agent sessions, lightweight signals and slash-command invocation
- 108 | q2 | uncommon | REA | LLM-judge dashboard and feedback hooks inside the client
- 109 | q1 | common | INF | [ABSENCE] No agent computer, browser or sandbox surface observed
- 110 | q1 | common | INF | [ABSENCE] No agent memory or per-person context model visible to users
- 167 | q4 | uncommon | REA | Graceful degradation when the client cannot render an agent action

**Collaborative artifacts**

- 111 | q4 | common | REA | Files: channel folders in SharePoint, chat files in OneDrive, with SharePoint limits
- 112 | q4 | unique | REA | Loop components and pages in messages and channels
- 113 | q4 | uncommon | REA | Collaborative meeting and chat notes editable by everyone
- 114 | q3 | common | REA | Whiteboard in meetings, chat tabs and shared screens
- 115 | q3 | common | REA | Lists, Planner, Wiki and Canvas tabs for structured work
- 116 | q2 | common | REA | Live event comment stream and Q&A

**Realtime & sync architecture**

- 117 | q4 | uncommon | SRC | Trouter: WebSocket push gateway shared by web client and native stack
- 118 | q4 | common | REA | Chat service REST with conversations, contacts, gap detection and degraded-sync mode
- 119 | q4 | uncommon | SRC | GraphQL client data layer with per-domain resolvers
- 120 | q4 | uncommon | SRC | Substrate signal service and search backend
- 121 | q4 | uncommon | SRC | Augloop and Fluid: real-time co-authoring and AI augmentation channels
- 122 | q4 | uncommon | SRC | Native media stack hosted behind a WebView2 bridge
- 123 | q3 | common | SRC | Local cache, local search index and offline posture
- 124 | q4 | uncommon | SRC | Sovereign and multi-cloud deployment configs
- 125 | q5 | common | DOC | Published scale limits and graceful feature degradation
- 126 | q4 | common | DOC | Encryption model: transit and rest, tenant keys, E2EE only for calls and meetings
- 127 | q4 | common | SRC | Activity signals across devices: web idle detection and user activity state
- 164 | q4 | uncommon | SRC | Worker and isolation topology for the web client

**Governance, security & compliance**

- 128 | q4 | uncommon | DOC | Retention and deletion for Teams messages via hidden compliance stores
- 129 | q5 | unique | DOC | Purview compliance suite for Teams: eDiscovery, legal hold, communication compliance, information barriers, audit
- 130 | q4 | uncommon | REA | Inline DLP: blocked message dialog with override-and-report options
- 131 | q4 | common | REA | Sensitivity labels across meetings, channels, shared channels and AI answers
- 132 | q4 | common | REA | Report a concern, block, spam requests and phishing-aware acceptance
- 133 | q4 | common | DOC | App and agent governance: allow, block, permission policies and compliance model
- 134 | q3 | uncommon | DOC | Compliance recording and media bots interact with E2EE and meeting privacy
- 135 | q5 | uncommon | SRC | Browser hardening in the shipped client: Trusted Types, strict CSP and isolation headers
- 136 | q4 | uncommon | SRC | Trusted UI and package-level isolation for sign-in and sensitive screens
- 137 | q4 | common | DOC | Data residency, sovereign clouds and AI data-boundary caveats
- 138 | q3 | unique | REA | Biometric voice and face profiles with export
- 139 | q4 | uncommon | REA | Diagnostics, privacy viewer and consented problem reports
- 140 | q4 | unique | DOC | AI interaction governance: Purview for agents, AI-use exclusions and web search switch
- 141 | q3 | uncommon | REA | Meeting content protections propagate to AI outputs only partly

**Client shell**

- 142 | q4 | uncommon | SRC | Thin native shell hosting a Microsoft-delivered web client in WebView2
- 143 | q4 | common | SRC | Windows integration: tray, jump list, autostart, badges and Phone Link
- 144 | q4 | common | SRC | Auto-update, rings and the ECS settings editor
- 145 | q4 | uncommon | SRC | Failure and recovery UX: error curtain, renderer watchdog and close prompts
- 146 | q4 | common | SRC | Multi-window pop-out chats and channels with window placement memory
- 147 | q4 | common | REA | Keyboard shortcuts with custom assignment and command-style slash menu
- 148 | q5 | common | SRC | Themes, accessibility and localisation at scale
- 149 | q4 | common | REA | Preview programs and staged rollout: Teams Labs, Insider and public preview
- 150 | q3 | unique | SRC | VDI optimisation bridge and end-of-availability notices
- 165 | q4 | common | SRC | Native sign-in, OneAuth and MSAL with desktop auth prompts

**UX patterns & layout**

- 151 | q5 | unique | REA | Unified three-pane chat and channels view with a switch-back escape hatch
- 152 | q4 | uncommon | REA | Inbox panel with inline thread replies and unread pills
- 153 | q4 | common | REA | Left app bar with pinned apps, accessible drag-and-drop and a store flyout
- 154 | q4 | common | REA | Zero states, coachmarks and lifecycle recommendation cards
- 155 | q5 | uncommon | REA | Accessibility-first list design: narrated state for every row
- 156 | q4 | uncommon | REA | Chat list settings that reshape the list: meeting chats section, muted section, private chats in meetings
- 157 | q3 | common | REA | Built-in feedback, what's new and campaign messaging
- 166 | q4 | common | REA | Contextual right-hand panes with a standard header
