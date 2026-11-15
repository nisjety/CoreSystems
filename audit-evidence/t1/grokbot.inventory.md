# Grok Bot 0.70.2 - Inventory (T1 scout, read-only)

Scope: Grok Bot desktop agent (productName "Grok Bot", internal package name "sand", version 0.70.2). Sources: installed copy at C:/Program Files/Grok Bot (resources/app.asar 42.9 MB, app.asar.unpacked, Grok Bot.exe) and the pre-extracted copy at scratchpad/extract/grokbot/ (package.json and dist/, 672 files, about 46 MB). Read-only: nothing installed, launched, built or modified; no per-user, credential, cookie or session stores opened. No secret values recorded (a few opaque telemetry ids and a credential file path name exist in code; values not recorded).

Provenance tags used below: REA = observed through REA analysis of a shipped artifact. SRC = read directly in package.json, bundles or binary strings (grep and node scans over dist). DOC = documentation (none shipped; only LICENSE files). INF = inference, basis stated. Confidence noted where not high.

Counts: renderer chunk files 532 of 594 assets; locale catalogs 35; Connect RPC services 7 (GrokBotService 309 methods, DashboardService 73, SandBoxService 55, AgentStoreService 24, AiService 8, AnalyticsService 8, BackgroundComposerService 7). Full method lists exported to scratchpad/tmp/svc_methods.json.

## 0. Method and REA usage (tools actually run)

- REA analyze_javascript_application on C:/Program Files/Grok Bot/resources/app.asar (format asar, detail summary, integrity_policy record-and-continue, max_heap_mb 2048, analysis_timeout_ms 900000). Ran once. Result: 616 relevant files, 600 parsed JS files, 41.1 MB text read, 7.07M AST nodes, 5,217 findings. Coverage partial: application graph 1,115 unknowns, semantic graph 13,215 unknowns. Integrity contradictions for 5 native files (cursor_proclist.node, sand_notification_settings.node, tree-sitter-bash win32-x64 .node, tree_sitter_runtime_binding.node, sand-webauthn-signer.exe); those bytes are untrusted. Parent evidence ev_2e557cc3fca76ed3a60cb306d239aeb8ceb38de4a443da2325746bda4cd61298. [REA]
- REA electron summary: 4 browser windows; 11 explicit web-preference sets; 6 context-bridge APIs; 15 IPC operations of which 13 are dynamic (channel expression unresolved) and 2 literal; 6 main handlers; 4 renderer listeners; 5 renderer transmissions; 2 sender-validation observations; 2 utility processes (0 entrypoints resolved); 2 native addon bindings (both resolved). [REA]
- REA inspect_analysis_view (modules page, parent evidence above): 766 nodes (600 javascript-asset, 166 javascript-module), coverage exhausted. Split: renderer/assets 545; electron-main/locales 105; deps 75; chrome-import-worker 23; electron-preload 9 (preload-avatar-source 4, preload-vnc 2, others 1 each); proto 3; main-app, main-core, main, onepassword-connection-service, local-exec-daemon/main.cjs and node-agent-coordinator/main.cjs 1 each. [REA]
- REA trace_application_feature, 4 seeds (string computer_use_screenshot exact; channel sand:vnc-session exact; channel sand:coordinator-port exact; api ipcMain contains). All returned no-match with a partial source graph. This is not evidence of absence. [REA]
- REA inspect_pe_resources: Grok Bot.exe (PE32+, machine 0x8664, 226,830,632 bytes; resource inventory complete; includes an ELECTRONASAR integrity resource, icon groups, and a version resource in en-US). sand-webauthn-signer.exe (PE32+, 276,776 bytes; no PE resources). [REA]
- REA tools not used: inspect_artifact, binary_overview, search_strings and list_strings (shared active target avoided), open_binary and binary_session (not needed).
- Verification pass (this continuation, after a usage-limit reset): REA was not re-run. The retained summary of the earlier analysis (ev_2e557cc3...) was re-read with inspect_analysis_view (summary view) and matches the figures above: 616 relevant files, 600 parsed JS files, 41.1 MB read, 5,217 findings, 4 browser windows, 11 explicit web-preference sets, 15 IPC operations (13 dynamic), 5 integrity contradictions, 1,115 application and 13,215 semantic unknowns. Re-checked by script: 159 section-3 bullets; 484 RPC entries (429 distinct names); 672 extracted files (about 46 MB); 35 locale folders; 532 chunk and 594 asset files; all 94 package.json dependencies appear in section 1a; no secret-like values found in this file. [REA; SRC]
- Version literals grepped from Grok Bot.exe bytes: Electron 42.1.0, Chrome 148.0.7778.97. [SRC]
- Other grep and node scans over dist: IPC literal names, sand:-prefixed channels, exposeInMainWorld names, RPC service descriptors, feature-gate names, env var names, English UI label candidates (about 57k unique candidate strings from renderer assets (mixed languages; English subset used)), tool-name literals, hostnames, original TypeScript source paths. [SRC]

## 1. Identity and stack

- Product: Grok Bot. Package "sand", version 0.70.2, description "Grok Bot desktop agent", author "SpaceXAI", private package, sandTrack "stable", sandBuiltAt 2026-10-11T05:25:27.938Z. [SRC package.json]
- Homepage field points to cursor.com. Code namespaces and hosts are Cursor-platform: proto package aiserver.v1.*, @anysphere/* workspace packages, api2, api3, api.origin, authentication and authenticator .cursor.sh hosts, CURSOR_* env names, and UI strings saying some settings are shared with Cursor or to sign in to Cursor for plugins. [SRC] Interpretation: Grok Bot is built on the Cursor platform stack with xAI/Grok product branding. [INF, high confidence on shared stack]
- Runtime: Electron 42.1.0, Chromium 148.0.7778.97 [SRC]; Windows x64 PE32+ [REA]. Windows build in this install (tree-sitter-bash win32-x64 prebuild; sand-webauthn-signer.exe). macOS-only code exists (Messages automation, Contacts helper error codes, messages-mac package) [SRC]; whether it is active on Windows: [INF, medium].
- Languages and frameworks: TypeScript compiled to CommonJS for main, preload, daemon and coordinator; browser ESM chunks for the renderer. React 19 (react ^19.3.0), Base UI, StyleX, TanStack Query 5, Tiptap 3 (ProseMirror), Lingui 5 (i18n), Zod 3, Connect RPC with protobuf-es v1. [SRC package.json, chunks]
- Renderer build: hashed chunk files (532 chunk-*.js) plus index.eager-app, index.eager-common, index.eager-platform and index.eager-vendor entries and index-*.js/css. No source maps in dist (grep for sourceMappingURL: 0 hits). [SRC]
- Original source layout surfaced in strings: src/electron-renderer/..., electron-renderer/client/sand-client.ts, client/send/send-journal.ts, client/persistence.ts, host/ports/telemetry.ts, node-agent-coordinator/webauthn/provider.ts, node-agent-coordinator/cookie-origin-approval/provider.ts. [SRC]
- Terminology: "Bot" is the product word for an agent. Bots have a computer called Box, can be personal or Team Bots, one can be the main Bot, and Bots join group chats (called Rooms in code). [SRC strings] Box meaning: [INF, medium-high; strings refer to a Bot-owned computer, box handoff, box_blocked, and reserve CURSOR_SANDBOX names for Box]

### 1a. Dependency list (package.json, grouped by capability)

package.json declares 94 dependencies (no devDependencies, no scripts). The grouped list below covers all 94; two @tiptap packages appear under short names. [SRC]

Workspace packages (workspace:*, all @anysphere/* unless noted):
- Agent runtime and sessions: agent, agent-client, agent-core, agent-exec, agent-kv, agent-store-sync, agent-summarization, agent-transcript, grok-bot, grok-bot-harness, chat-inference, chat-inference-proto, model-selection, mcp-core, mcp-agent-exec, shell-exec, local-exec, virtual, context, context-rpc, draft-revision, voice-call-harness, browser-smoke, orbit-client, xai-commerce-prepaid (payments), dune, dune-reliability.
- Plugins, config, content: cursor-plugins, cursor-config, marketplace-category, link-preview, file-kind-icons, canvas-shared, messages-mac, codebase-telemetry.
- Telemetry and proto: analytics-client, analytics-types, metrics, otel-proto, proto, redacted-protos.
- Shared: constants, utils, ui, @sand/client, @sand/shared.

Third-party (versions as declared in package.json):
- RPC and transport: @connectrpc/connect ^1.6.1, @connectrpc/connect-node ^1.6.1, @bufbuild/protobuf 1.10.1, ws 8.21.3.
- Telemetry: @sentry/electron 7.2.0, @sentry/node-core 10.17.0, @statsig/js-client ^3.20.0, @opentelemetry/api ^1.9.0, @opentelemetry/core ^2.2.0, @opentelemetry/resources ^2.2.0, @opentelemetry/sdk-trace-node ^2.2.0, @opentelemetry/exporter-trace-otlp-proto ^0.208.0.
- UI framework and interaction: react ^19.3.0, react-dom ^19.3.0, @base-ui/react 1.8.0, @stylexjs/stylex 0.18.3, @tanstack/react-query 5.100.9, motion ^12.18.1, @floating-ui/dom ^1.7.6, @dnd-kit/core ^6.3.1, @dnd-kit/sortable ^10.0.0, @dnd-kit/utilities ^3.2.2, react-hotkeys-hook ^5.1.0.
- Rich text and editor: @tiptap/core, @tiptap/react, @tiptap/pm, @tiptap/html, @tiptap/starter-kit, @tiptap/suggestion ^3.14.0; @tiptap/extensions 3.14.0; @tiptap/extension-link, extension-mention, extension-placeholder ^3.14.0.
- i18n and emoji: @lingui/core ^5.9.5, @lingui/react ^5.9.5, emojibase-data 17.0.0.
- Document and markdown rendering: react-markdown ^10.1.0, remark-gfm ^4.0.1, remark-math-extended ^6.1.0, katex ^0.16.45, mermaid ^11.16.0, pdfjs-dist 5.4.296, mammoth ^1.12.0, xlsx ^0.18.5, adm-zip ^0.5.16.
- Native and OS: cursor-proclist (file:../vscode/native-modules/cursor-proclist), sand-notification-settings (file:native/notification-settings), dist/deps tree-sitter and tree-sitter-bash (bash command parsing), web-tree-sitter (wasm), node-gyp-build, node-addon-api. Of these, tree-sitter, tree-sitter-bash, web-tree-sitter, node-gyp-build and node-addon-api are not declared in package.json; they ship under dist/deps (runtime-deps-manifest.json).
- Other: zod ^3.25.0.

## 2. Directory and module map (top 2-3 levels)

- package.json (4.2 KB): manifest; main = dist/electron-main/main.cjs.
- dist/electron-main/ (6.1 MB)
  - main.cjs (9 KB): bootstrap entry.
  - main-core.cjs (854 KB): core main-process logic (boot, host, gateway, sessions, safeStorage, reverse channels, outbox references).
  - main-app.cjs (2.58 MB): app wiring (windows, IPC handlers with trusted-requester checks, webview guest policy, Slack, Teams and MCP OAuth callbacks, sand:* channels).
  - proto.cjs (1.2 MB): generated protobuf and Connect descriptors (aiserver.v1.* services and messages).
  - chrome-import-worker.cjs (11.5 KB): Chrome cookie import worker (node:sqlite).
  - onepassword-connection-service.cjs (104 KB; also unpacked in install): 1Password connection service.
  - locales/ (35 locale folders, each messages.cjs): Lingui catalogs for main-process strings (af, ar, bn, cs, da, de, el, es, fi, fr, ha, he, hi, hu, id, it, ja, ko, mr, nb, nl, pl, pt, ru, sv, sw, ta, te, th, tr, uk, ur, vi, zh-CN, zh-TW).
- dist/electron-preload/ (188 KB)
  - preload.cjs (107 KB): main bridge exposing window.desktop, coordinatorPort, sandRenderTrace, desktopMount; sand:* IPC.
  - preload-vnc.cjs (45 KB): noVNC and RFB remote-desktop viewer bridge.
  - preload-webview.cjs (6.5 KB), preload-desktop-mount-panel.cjs (12.7 KB), preload-avatar-source.cjs (5.6 KB).
- dist/local-exec-daemon/main.cjs (3.5 MB): local execution daemon (shell with sandbox policies, MCP server handling, computer-use input tools, SCM and Messages helper error codes, reverse local-exec channel client).
- dist/node-agent-coordinator/main.cjs (882 KB): coordinator utility process (utilityProcess.fork, service name sand-node-agent-coordinator); agent roster, pod and box lifecycle states, WebAuthn provider, cookie-origin approvals, RFB connect, teach recording, voice-call states.
- dist/native/sand-webauthn-signer.exe (277 KB): passkey and WebAuthn signer helper (path overridable via SAND_WEBAUTHN_SIGNER_PATH).
- dist/deps/ (2.8 MB): native addons and wasm: cursor-proclist (process listing .node), sand-notification-settings (.node), tree-sitter (plus runtime binding .node), tree-sitter-bash (win32-x64 .node), web-tree-sitter (tree-sitter.wasm), node-gyp-build, node-addon-api, runtime-deps-manifest.json.
- dist/renderer/ (32 MB)
  - index.html; src/electron-renderer/desktop-mount/panel/panel.html (the only source-like file).
  - assets/ (594 files): 532 chunk-*.js feature and vendor chunks (feature chunk groups listed in section 3); index.eager-app (renderer app entry); index.eager-common, index.eager-platform, index.eager-vendor; KaTeX and XMono/X-VF woff2 fonts; 16 integration brand logos (Apollo, Ashby, Box, Calendly, Canva, Clay, Databricks, Mailchimp, Nooks, QuickBooks, Rippling, Salesforce, Snowflake, Tableau, Workday, ZoomInfo); 5 wav UI cues (open-blip, tick, double-tick, chime-a, chime-b); demo-computer-wallpaper jpg; gallery-loop mp3; pdf.worker.min.mjs; dune-render-trace-hook js; desktop-mount-panel js and css; fullscreen-file-viewer-body css; markdown-math-katex css.
- Install-only: Grok Bot.exe (227 MB), resources/app.asar (42.9 MB), resources/app.asar.unpacked/dist/ (deps native files, electron-main/onepassword-connection-service.cjs, native/sand-webauthn-signer.exe), Electron and Chromium runtime (ffmpeg, libEGL, libGLESv2, Vulkan and SwiftShader, dxcompiler, d3dcompiler), locales/, LICENSE files. [SRC]
## 3. Feature candidates (grouped by taxonomy; pointers in brackets)

Pointer key: main-app = dist/electron-main/main-app.cjs; main-core = dist/electron-main/main-core.cjs; proto = dist/electron-main/proto.cjs; daemon = dist/local-exec-daemon/main.cjs; coord = dist/node-agent-coordinator/main.cjs; preload = dist/electron-preload/preload.cjs; renderer chunk <name> = dist/renderer/assets/chunk-<name>-*.js; RPC = aiserver.v1 method (full lists in scratchpad/tmp/svc_methods.json); string = English UI string candidate (renderer assets or locales).

### Category 1. Messaging core
- Bot chat: send a user message, interrupt a running turn, start a fresh conversation, roll back a conversation, check send status. RPC GrokBotService: SendGrokBotUserMessage, InterruptGrokBotAgentRun, StartFreshGrokBotConversation, RollBackGrokBotConversation, GetGrokBotSendStatus. [SRC RPC; renderer chunk chat-*]
- Agent-originated messages: SendGrokBotAgentMessage; agent email sending SendGrokBotEmail. [SRC RPC]
- Drafts before sending: SendGrokBotDraft, DiscardGrokBotDraft, "Disable drafts" setting (SetGrokBotAgentDraftingDisabled), draft-composer-bubble and draft-send-error chunks; send journal with nonces and digests (client/send/send-journal.ts). [SRC]
- Reactions and feedback on Bot messages: ReactToGrokBotMessage ("Add reaction", "Quick reactions"); VoteGrokBotFeedback. [SRC RPC, string]
- Replies and threads: "Reply in thread", "Open thread" and "In reply to" strings; thread-card and settled-message-card chunks. [SRC string, chunk]
- Mentions and references: reference-menu chunk (@-style references); prompt-editor chunk (Tiptap-based composer); Slack and Teams mentions can wake a Bot. [SRC]
- Unread and read state: "Mark as Unread", "More unreads", "No unread Bots"; events mark_read and mark_unread (coord); MarkGrokBotTasksSeen. [SRC string, coord]
- Pinning and sidebar placement for Bots: "Pinned Bots" label; events move_to_section, move_to_new_section, hide_from_sidebar, unhide_from_sidebar (coord). [SRC string, coord]
- Attachments: chunked and multipart uploads (BeginGrokBotAttachmentUpload, PresignGrokBotAttachmentUploadParts, UploadGrokBotAgentAttachmentChunk, CompleteGrokBotAttachmentUpload, AbortGrokBotAttachmentUpload); reads (ReadGrokBotAgentAttachmentChunk, ResolveGrokBotAttachmentReads); "attachment too large" error (attachment_too_large). [SRC RPC, daemon, coord]
- Image understanding: StartGrokBotImagePrecaption. Image generation: GenerateImage tool and AiService.RunGenerateImage. [SRC RPC]
- Voice memos: voice-memo-bubble chunk; playback speed labels 0.5x to 2x; "Jump to this word in the voice memo" (transcript-linked playback). [SRC string, chunk]
- Transcripts in chat: CommitGrokBotTranscriptEntries, ListGrokBotTranscriptEntries, WatchGrokBotTranscripts (server stream); "Conversation transcript" and "Bot exchange transcript" strings. [SRC RPC, string]
- Typing and presence: "Typing" string; ReportGrokBotClientPresence; ListGrokBotAgentStatuses (presence and status of Bots; not human presence). [SRC RPC, string; low confidence on human presence]
- Human-to-human messaging (DMs, channels, edit or delete, forward, polls, bookmarks, read receipts, scheduled send, pinned messages, do-not-disturb): not found in RPC names or English strings (English candidate list: zero hits for poll, bookmark, pin message, edit message, delete message, read receipt, do not disturb and mute; the only "Pin" hits are a drop-zone label or non-English text; "scheduled" appears only for computer updates). [SRC absence, medium] Teammates interact through Bots, group chats (Rooms) and Slack or Teams. [SRC absence, medium; INF: human chat is delegated to Slack, Teams and group chats]

### Category 2. Org and membership
- Teams and invites: GetTeams, GetMe, GetTeamMembers, GetTeamGroups, SendTeamInvite, GetTeamInviteLink (DashboardService); "Invite teammates" and "Add teammates by email or share an invite link" strings. [SRC RPC, string]
- Team Bots: ListGrokBotTeamAgents, CloneGrokBotAgentToTeam, DeleteTeamGrokBot, AdminListGrokBotTeamAgents; "New team bot" string; team-bot-* chunks (details, overview, setup, service accounts, custom plugins, plugins dialog, context dialog, carry-over plugin selection, teammate intro). "A Team Bot can't be your main Bot." [SRC]
- Service accounts and team setup: team-bot-service-accounts-dialog and team-bot-service-account-tiles chunks; team-setup-* chunks (new secret dialog, new skill dialog, pills, row). [SRC chunk names]
- Personal Bots and the main Bot: SetGrokBotMainAgent, EnsureGrokBotDefaultMainAgent, EnsureGrokBotCursorMainBot; IPC channel sand:choose-main-agent; "A channel cannot be the main bot." [SRC]
- Managers and access control: AddGrokBotAgentManager, RemoveGrokBotAgentManager, ListGrokBotAgentManagers; AddGrokBotAgentAccessPrincipal, RemoveGrokBotAgentAccessPrincipal, ListGrokBotAgentAccessPrincipals, GetGrokBotAgentAccess, UpdateGrokBotAgentAccess; GetGrokBotAgentLinkAccess (link access). [SRC RPC]
- Visibility and publishing: SetGrokBotAgentVisibility, PublishGrokBotAgent, UnpublishGrokBotAgent; strings "Admins publish" and "Draft: only you can see it until you publish". [SRC RPC, string]
- Team privacy mode: GetUserPrivacyMode; a team Privacy Mode string says it can block the app. [SRC RPC, string]
- Team policy and admin settings: provider-team-policy and provider-local-tool-permission chunks; use-team-bot-access hook; GetTeamAdminSettingsOrEmptyIfNotInTeam; codes team_policy_unavailable, team_required, account_required. [SRC]
- Admin console and team operations: 41 Admin* RPCs in GrokBotService (the 15 Admin* names in SandBoxService also appear there) (for example AdminListSandAgents, AdminGetSandAgentTranscriptPage, AdminBreakGlassDeleteSandBoxPod, AdminHibernateSandBox, AdminSnapshotSandBoxStore, AdminRestoreSandBoxStoreSnapshot, AdminUpdateSandBoxHost, AdminGetSandBoxHostStatus); team-member sandbox operations ListTeamMemberSandBoxes, KillTeamMemberSandBox, RecreateTeamMemberSandBox, StartBulkTeamMemberSandBoxOperation, GetBulkTeamMemberSandBoxOperationStatus. [SRC RPC]
- Team setup manifests (sandbox setup per team and per team group): ListTeamSandSetupManifests, SaveTeamSandSetupManifest, DeleteTeamSandSetupManifest, ListTeamGroupSandSetupManifests, SaveTeamGroupSandSetupManifest, DeleteTeamGroupSandSetupManifest; per-user ListSandSetupManifests. [SRC RPC]
- Plans, seats and trials: codes seat_revoked, no_plan, plan_expired, pro_plus, enterprise_grok_bot_trial_cap, unpaid_invoice; GetSandAccessStatus, GetSandTrialClaimStatus, CancelSandTrial; onboarding-flow chunk. [SRC]
- Terms acceptance: MarkGrokBotTermsSeen, GetGrokBotTermsSeen. [SRC RPC]

### Category 3. Calls and meetings
- Voice chat with a Bot: VoiceCallHarnessSession and VoiceCallHarnessTool (GrokBotService); MintSandVoiceCallSecret; strings "End the current voice chat?" and a channel-closed message after a call ends; coord and daemon states mic_granted, mic_denied, mic_timeout, voice_processing_off, end_the_call, stay_silent, keep_waiting and barge-in drop reasons. [SRC]
- Speech services: AiService.StreamStt (bidirectional streaming speech-to-text), TranscribeAudio, TextToSpeech. [SRC RPC]
- Meetings: Bots join and leave meetings (PullGrokBotMeetings, ReportGrokBotMeetings, ListGrokBotServerMeetings, LeaveGrokBotServerMeeting, GetGrokBotMeetingRecord, ListGrokBotAgentMeetings, SetGrokBotAgentMeetingsVisibility); meeting-consent, meeting-prep and meeting-chat-draft chunks; meeting-server-list and meeting-server-leave events; consent strings for one meeting and for all meetings. Meeting platforms named in locale-catalog copy: Google Meet and Zoom (copy tells users to send a Bot a meeting link so it joins); no Webex or "Microsoft Teams meeting" string found. [SRC string; join behaviour not verified]
- Meeting intelligence (admin side): AdminGetGrokBotMeetingTranscript, AdminGetGrokBotMeetingSummaryVersions, AdminLookupGrokBotMeetings, AdminLookupGrokBotMeetingsByAgent, AdminGetGrokBotMeetingJoinConsent, AdminMintGrokBotMeetingAvatarLinks; summaries are versioned. [SRC RPC]
- Recording by a Bot: token recordingAgentName appears 36 times in renderer chunks; "Enable Recording" and "Discard recording" strings. Scope and consent UI not verified. [SRC, low to medium]
- Meeting prep: marketplace template copy says a Bot builds a prep pack from calendar, CRM and Slack before each meeting. [SRC string; template copy, not product behavior evidence]
- Calendar triggers: string "At the followed calendar event's start" (calendar automation trigger); calendar hosts referenced (Google Calendar). [SRC]
- Live remote desktop of a Bot computer: noVNC/RFB viewer (preload-vnc.cjs; rfb_connect and rfb_disconnect in coord, main-app and preload); VNC session channels sand:vnc-session, sand:vnc-liveness, sand:vnc-interactive, sand:vnc-accepts-input, sand:vnc-viewer-visible, sand:vnc-host-key; desktop strings "Desktop starting" and "Can't reach the screen"; screenshot-plus-live-URL wording for a Bot computer. [SRC]
- Whiteboard, huddles, screen share and AI recap as meeting features: not found (zero English-candidate hits for whiteboard, huddle and screen share; "captions" appears only as a video-viewer prop). [SRC absence, medium]

### Category 4. Search and discovery
- Email search: SearchGrokBotEmailThreads, ReadGrokBotEmailThread, ReadGrokBotEmailAttachment. [SRC RPC]
- Library, memory and context: ListGrokBotLibraryDocs; ListGrokBotUserBotMemories; GetGrokBotTeamContextSummary. [SRC RPC]
- Conversation search: token search_conversations (coord). [SRC, low]
- Marketplace and plugin discovery: marketplace-browse-model and marketplace-category chunks; ListPublicGrokBotMarketplaceListings, GetPublicGrokBotMarketplaceListing; GetTeamPluginPopularity; "Find plugins in the marketplace" string. [SRC]
- Canvas list: ListUserCanvases. [SRC RPC]
- Messages search (macOS): "Lets Grok Bot search your texts" string tied to the Messages automation; Windows availability [INF, medium]. [SRC string]
- Global search: legacy menu search input (LegacyMenu.SearchInput) and generic search strings; no evidence of semantic search or an AI answer surface. [SRC, low]

### Category 5. Notifications and attention
- OS notifications: native module sand-notification-settings (dist/deps .node) and Electron Notification (1 reference); ReportGrokBotNotificationPermission; gate grok_bot_notify_active_device_only (notify only on the active device). [SRC]
- Attention prompts: UI prompt copy offering Bot notifications for items that need attention or review; NotifySandAgentTurnFinished (RPC). [SRC string; RPC]
- Tasks inbox and async tasks: ListGrokBotTasks, DismissGrokBotTask, MarkGrokBotTasksSeen, KeepGrokBotTaskList, ListGrokBotAgentTasks, MarkGrokBotAgentTaskRead, MarkGrokBotAgentTaskDone, ListGrokBotAgentTodos; "Async tasks" string; show_async_tasks event (coord). [SRC RPC, string, coord]
- Digests and briefings: "Channel Digest" string; "Works overnight and preps your morning digest"; room-join briefing and catch-up summary strings. [SRC string]
- Badges: unread badge logic and a muted-channel badge fix string. [SRC string]
- Activity reporting: ReportGrokBotDesktopActivity; ReportSandBoxHostState. [SRC RPC]
- Sound cues: five wav files (open-blip, tick, double-tick, chime-a, chime-b). [SRC]

### Category 6. Apps and extensibility
- Plugin marketplace: ListMarketplacePlugins, InstallUserPlugin, UpdateUserPluginInstall, UninstallUserPlugin, GetEffectiveUserPlugins, ListMarketplaces, GetManagedSkills (DashboardService); chunks plugin-detail-view, plugins-rows, native-plugin-detail, native-plugin-state; strings "Add custom plugin" and "Add from Plugins". [SRC]
- Per-Bot plugins: GetGrokBotAgentPlugins, SetGrokBotAgentPlugins, SetGrokBotAgentPluginVariables, UpdateGrokBotAgentMarketplace, GetGrokBotAgentPluginFile; team-bot-custom-plugins and team-bot-plugins-dialog-list chunks. [SRC]
- Plugin manifest format: .cursor-plugin/marketplace.json referenced in bundle. [SRC]
- Skills: SKILL.md format referenced; ListGrokBotAgentSkills, AddGrokBotAgentSkill, UpdateGrokBotAgentSkill, RemoveGrokBotAgentSkill; PublishGrokBotUserSkillsSnapshot, InvalidateGrokBotUserSkillsCache; "Add new skill" string; "Skills teach your Bot how to work..." string; sand-auto-review-instructions chunk. [SRC]
- Teach by demonstration: "Teach a task" string; teach-recording-controls and teach-recording chunks; teach_recording events in main-app and coord. [SRC]
- MCP servers: GetGrokBotUserMcpSettings, SetGrokBotUserMcpSettings, GetGrokBotMcpToolPermissions, SetGrokBotMcpToolPermissions; DashboardService GetMcpConfig, SetMcpConfig, GetAvailableMcpServers, GetPluginMcpConfig, CheckHttpMcpStatus, ListSandMcpTools, ExecuteSandMcpTool, ReadSandMcpResource; MCP OAuth per account: CompleteMcpOAuth, ValidateMcpOAuthTokens, DeleteMcpOAuthToken, RenameMcpOAuthAccount, DeleteMcpOAuthAccount; daemon fields mcp_version, mcp_source, mcp_server, mcps_root, configured_servers, mcp_auth; deep link grokbot://mcp/oauth/callback. [SRC]
- MCP apps: gate sand_mcp_apps; ReadGrokBotMcpAppDocument. [SRC]
- Native connectors (OAuth) and brand logos: 16 brand logo assets, Microsoft Teams brand logo chunk, native-connector-card and connector-auth-telemetry chunks; connector grants ResolveGrokBotConnectorGrant and ClearGrokBotConnectorAlwaysAllow (always-allow state); hosts referenced for Notion, Linear, Figma, Canva, Miro, Coda, Airtable, Hex, Box, Google Drive, Google Docs, Google Calendar, Gmail, OneDrive, GitHub, GitLab, Bitbucket, Azure DevOps. Which connectors are live versus planned: [INF, low]. [SRC]
- Slack bot app: StartGrokBotSlackConnect, GetGrokBotSlackInstallState, InstallGrokBotSlackApp, ReinstallGrokBotSlackApp, UninstallGrokBotSlackApp, AdminRetryGrokBotSlackAppRemoval; DashboardService GetSlackInstallUrl, GetSlackUserSettings, UnlinkSlackAccess; Bot can receive Slack DMs, be mentioned in channels, and wake on reactions; "Slack channels and DMs (coming soon)" for outbound messaging; Slack admin approval strings. [SRC]
- Microsoft Teams bot app: GetGrokBotTeamsInstallState, StartGrokBotTeamsConnect, InstallGrokBotTeamsApp, ReinstallGrokBotTeamsApp, UninstallGrokBotTeamsApp, AddGrokBotTeamsAppToTeam; a Bot can live in one Teams organization; Teams admin approval strings. [SRC]
- Discord: "Discord servers and DMs (coming soon)" string only; no Discord RPC found. [SRC, low]
- Email inboxes for Bots: CreateGrokBotEmailInbox, ListGrokBotEmailInboxes, CheckGrokBotEmailAddressAvailability, DeleteGrokBotEmailInbox, SendGrokBotEmail, ReadGrokBotEmailAttachment; admin send state, limits, thresholds and suppressions. [SRC RPC]
- Webhooks: CreateAutomationWebhookApiKey (DashboardService); webhook URL and key strings describing a key sent with each request. [SRC]
- Automations: ListGrokBotAgentAutomations, ListGrokBotAccountAutomations, SetGrokBotAgentAutomationEnabled, DeleteGrokBotAgentAutomation; trigger wording covers Slack messages, mentions and reactions, GitHub PRs, comments, issues and CI, and calendar events; automation-detail-panel chunk; "routines" strings. [SRC]
- Templates and sharing: CreateGrokBotTemplate, ListGrokBotTemplates, DeleteGrokBotTemplate, SetGrokBotTemplateVisibility, ActivateGrokBotTemplateVersion, GetGrokBotTemplateVersion, GetGrokBotTemplateExportPolicy, GetPublicGrokBotTemplate, GetGrokBotTemplateImportDetails, CreateGrokBotAgentFromTemplate, GetGrokBotTemplateForSourceAgent; bot-template-* chunks; "Share as template" and "A template link or share ID" strings. [SRC]
- Marketplace operator tools (internal): CreateGrokBotMarketplaceListingInternal, CreateGrokBotMarketplaceCreatorInternal, CreateGrokBotMarketplaceCategoryInternal, CreateGrokBotChangelogInternal, PreviewGrokBotMarketplaceSourceInternal and matching list, get, update and delete calls. [SRC RPC]
- Forms and widgets rendered by Bots: RespondGrokBotWidget, DismissGrokBotWidget, SubmitGrokBotUserForm, DismissGrokBotUserForm; user form vault: ListGrokBotUserFormVaultEntries, UpsertGrokBotUserFormVaultEntry, DeleteGrokBotUserFormVaultEntry, ListGrokBotUserFormVaultKeys. [SRC RPC]
- CLI and public SDK: none found. Process configuration uses SAND_* environment variables and deep links. [SRC absence, medium]
### Category 7. Agents in the room
- Personal Bots and cloud computers (Box): EnsureSandBox, EnsureSandBoxWindow, RecreateSandBox, ForceRecreateSandBox, GetSandBoxRunState, ListSandBoxes, ReportSandBoxHostState; hibernation and upgrade scheduling: AdminHibernateSandBox, GetSandBoxUpgradeSchedule, ScheduleSandBoxUpgrade, RescheduleSandBoxUpgrade, CancelSandBoxUpgrade; strings "Computer update scheduled", "computer update already in progress", automatic computer updates. [SRC RPC, string]
- Box handoff: EndGrokBotBoxHandoff; "Bot failed to resume after box handoff" string; box_blocked and pod_hibernated states (coord). [SRC]
- Local machine ("this computer") execution: a desktop registers as a machine (RegisterSandMachine, ListSandMachines, UpdateSandMachineLabel, UpdateSandMachineLocalToolPermission, RemoveSandMachine, GetSandMachineMessagesEnabled, UpdateSandMachineMessagesEnabled); strings that offer to let Bots run tasks on the user's own computer as well as the Bot's computer, and that say a Bot on the user's machine cannot share a room with a teammate's Bot. [SRC RPC, string]
- Reverse channels to the desktop: local-exec reverse channel (desktop heartbeats are checked against a liveness window; refusal message when no desktop heartbeats) and a reverse WebAuthn channel (refusal message when no desktop is registered). [SRC string, main-core]
- Computer-use tools on the local desktop: computer_use_screenshot, computer_use_click, computer_use_mouse_move, computer_use_mouse_button, computer_use_drag, computer_use_scroll, computer_use_typing, computer_use_press_key, computer_use_check_permissions. [SRC; local-exec-daemon/main.cjs]
- Agent tool set (names seen in bundles, reference counts): Shell (31), Delete (13), Subagent (10), WebSearch, WebFetch, UpdateMemory, Read, Task, GenerateImage, CallMcpTool, send_message; message parts tool_use and tool_result. [SRC]
- Shell execution with sandbox policies (daemon): sandbox types insecure_none, workspace_readonly and workspace_readwrite; network policy; write-protection mapping; read boundary; captureDenies with sandbox_denies events; bash command classification via tree-sitter-bash (categories such as heredoc, command substitution, process substitution, herestring and file redirect). [SRC; local-exec-daemon/main.cjs, dist/deps]
- Tool approval states: always_allow, needs_approval, blocked; approval modes auto, manual, ask, always; ResolveGrokBotLocalToolPermission; ResolveGrokBotAutoReviewApproval; auto-review rules; ask_admin_role. [SRC]
- Subagents: ListGrokBotSubagents; "Couldn't load subagents" and related strings. [SRC RPC, string]
- Background agents and workers: BackgroundComposerService (ListWorkersForManager, GetTurnSummaryBackgroundComposer, ListBackgroundComposerArtifacts, GetOptimizedDiffDetails, GetPullRequestMergeStatus, GetBackgroundComposerInfo, GetBackgroundComposerUserSettings); manager and worker terms. [SRC RPC]
- Multi-agent rooms and group chat: CreateGrokBotRoom, SetGrokBotRoomMembers, AddGrokBotRoomPeople, RemoveGrokBotRoomPeople, RequestGrokBotRoomMemberTurn, CancelGrokBotRoomMemberTurn, DeliverGrokBotRoomMemberTurnResult; group chat strings ("Create group chat", "Add to group chat", "Group chats", "Add people or groups"); coord turn states work_overheard, work_landed, stay_silent, turn_ended; usage notice: group chats consume more usage because several Bots process each message. [SRC RPC, string, coord]
- Human-agent handoff and control: EndGrokBotBoxHandoff; InterruptGrokBotAgentRun; ResolveGrokBotCredentialRequest; approval prompts. A "take over" action label was not found. [SRC; absence noted]
- Agent memory: ListGrokBotUserBotMemories; PromoteGrokBotMemoriesToTeam; GetGrokBotTeamContextSummary; memories named in Slack and Teams copy; UpdateMemory tool. [SRC RPC, string]
- Scheduled and background work: automations (see category 6); temporal agents (CreateGrokBotTemporalAgent; gate sand_create_temporal_agents; semantics unverified); "routines" and schedule-timezone strings (the timezone Grok Bot uses for schedules and reminders). [SRC RPC, string]
- Agent permissions and secrets: team secrets with types (Slack token, GitHub token, Google API key, Microsoft Entra ID, Email, CLI credentials, generic secret); string saying secrets are never shown to teammates; ListGrokBotSecrets, PutGrokBotSecret, DeleteGrokBotSecret, SubmitGrokBotSecret; vault credentials ListGrokBotVaultCredentials, PutGrokBotVaultCredential, DeleteGrokBotVaultCredential; credential requests ResolveGrokBotCredentialRequest. [SRC RPC, string]
- 1Password integration: BeginOnePasswordConnection, CompleteOnePasswordConnection, GetOnePasswordState, SyncOnePasswordConnections, DeleteOnePasswordConnection, StartOnePasswordOAuth, CompleteOnePasswordOAuth, StoreOnePasswordIntegrationKey, ApproveOnePasswordCredentialRequest, DenyOnePasswordCredentialRequest, GetGrokBotOnePasswordExtensionDownload; onepassword-connection-service.cjs; string about an active 1Password CLI integration. [SRC RPC, string]
- Payments and spend for Bots: RaiseGrokBotVirtualCard and ResolveGrokBotVirtualCardApproval (virtual card approval); ListGrokBotStripeLinkPaymentMethods; BeginGrokBotSpendApproval and FinishGrokBotSpendApprovalAssertion (passkey-backed spend approval). [SRC RPC, string]
- Spend limits and usage: GetHardLimit, SetHardLimit, GetDailySpendByCategory, GetCurrentPeriodUsage, GetSandUsageStatus, GetAggregatedUsageEvents, GetMonthlyBillingCycle; prepaid billing with auto top-up (GetTeamPrepaidBilling, SetTeamPrepaidAutoTopUp, GetUserPrepaidBilling, SetUserPrepaidAutoTopUp); top-up-modal-host chunk. [SRC RPC]
- Financial account linking (observed RPC names only): ListFinancialConnections, ListFinancialAccounts, StartFinancialLink, DeleteFinancialConnection (DashboardService). [SRC RPC]
- Agent identity: agent-avatar-editor and agent-character-picker chunks; avatar color names (Red, Orange, Yellow, Green, Cyan, Blue, Violet, Magenta, Brown, Gray, Black); avatar upload and fetch; UploadUserProfilePicture. [SRC]
- Agent runtime settings and model choice: GetGrokBotRuntimeCapabilities, GetGrokBotUserRuntimeSettings, UpdateGrokBotUserRuntimeSettings; string "Choose which AI model Grok Bot thinks with"; AiService.AvailableModels; model-selection package; codes unified_ultra and unsupported_model. [SRC RPC, string]
- Voice-call tool calls by a Bot: VoiceCallHarnessTool. [SRC RPC]

### Category 8. Collaborative artifacts
- Canvases: ListUserCanvases, GetCanvasMetadata, GetCanvasPayload, SetCanvasVisibility, SetPageAddress (published page address), RenameCanvas; canvas-shared package; cursor-canvas layout options (gutter, content column, top and bottom inset, viewport height). [SRC RPC, renderer or main flags]
- Group chat documents and canvases: ReadGrokBotGroupChatDocument, ReadGrokBotGroupChatCanvas. [SRC RPC]
- Agent store (versioned document store): AgentStoreService ListAgentStores, ListAgentStoreEntries, ReadAgentStoreFile, WriteAgentStoreDocument, SetAgentStoreDocumentVisibility, MintAgentStoreToken, ListAgentStoreFiles, ListAgentStoreDirectory, CheckAgentStoreVersions, presigned reads and writes, multipart complete and abort, file locks (Acquire, Renew, Release, Get), DeleteAgentStoreFiles, RmdirAgentStore, UndeleteAgentStoreFiles, ListAgentStoreTombstones, ShareAgentStore, UnshareAgentStore, ListSharedAgentStores. [SRC RPC]
- Agent files: BeginGrokBotAgentFileUpload, FinishGrokBotAgentFileUpload, ListGrokBotAgentFiles, DeleteGrokBotAgentFile. [SRC RPC]
- Library docs: ListGrokBotLibraryDocs. [SRC RPC]
- Diagrams: Mermaid diagram families (flowchart, sequence, class, state, ER, gantt, journey, git graph, pie, quadrant, requirement, C4, architecture, block, sankey, xy chart, kanban, timeline, mindmap, cynefin, wardley, ishikawa, swimlanes, venn, packet and others); PNG export strings for diagrams. [SRC]
- Documents and media: PDF (pdfjs-dist with worker), DOCX (mammoth), XLSX (xlsx), Markdown and MDX (react-markdown, mdx chunk), file viewers (fullscreen-file-viewer-body, document-page, file-stage, get-file-icon), code highlighting via Shiki (chunk-shiki-token-cache; 9 renderer files mention Shiki; language count not verified). [SRC]
- Code review and PR chat: EnsureGrokBotPullRequestChat, ListGrokBotPullRequestChatEntries, SendGrokBotPullRequestChatMessage; copy describing reading and acting on PRs, issues, checks and code; diff chunk; GetOptimizedDiffDetails; GetPullRequestMergeStatus. [SRC RPC, string]
- Source control connection: PrepareGithubConnectFlow, CompleteGithubConnectFlow, DisconnectGithub, GetGithubRepoAccessStatus; source-control error codes (scm_token_unavailable, scm_token_rejected, scm_repo_not_accessible, scm_org_blocked, scm_not_connected); azure_devops token; copy asking the user to connect a source-control integration so cloud agents can reach their repos. [SRC RPC, string, daemon]
- Tasks and to-do lists: ListGrokBotAgentTodos; KeepGrokBotTaskList. [SRC RPC]
- Templates as shareable artifacts: bot-template-share-card-frame chunk and template share strings. [SRC]
- Review and approval flows for artifacts: approval states, Auto-Review, "Ready for review" strings. [SRC]

### Category 9. Realtime and sync architecture
- RPC transport: Connect (connect-node in main; Connect clients in renderer chunks) with protobuf-es v1 messages; gRPC-web transport referenced (createGrpcWebTransport, 1 reference). [SRC]
- WebSocket: ws 8.21.3 in main; 82 WebSocket references across bundles; socket_open, socket_error and socket_close events (coord). [SRC]
- Server-sent events and long polling: text/event-stream (8 references); long_polling and server_sent_events tokens; server-streaming RPCs WatchGrokBotTranscripts, WatchSandBoxMigration, AdminWatchSandBoxMigration, WatchGrokBotUserComputerRequests, OpenGrokBotUserComputerRequest. [SRC RPC, daemon]
- Polling fallback for computer requests: PollGrokBotUserComputerRequests; SubmitGrokBotUserComputerResponses; IssueGrokBotUserComputerCredential. [SRC RPC]
- Reverse channels and heartbeats: local-exec reverse channel and reverse WebAuthn channel; gateway configured through SAND_HOST_GATEWAY_URL and gateway.json; flags SAND_DISABLE_GATEWAY_STREAM_LIVENESS and SAND_DISABLE_GATEWAY_HEALTH_TTL. [SRC]
- Offline and resume: queued_offline state, replay_event, resume window values (resume_upper_s, resume_lower_s), in_flight_cap, queue_overflow, dropped_superseded and dropped_line_dead states; string "Bot failed to resume after reaction". [SRC coord, string]
- Send reliability: send journal (client/send/send-journal.ts) with nonce and digest acceptance; outbox JSONL (outbox.v1.jsonl in main-core); client persistence (client/persistence.ts). [SRC]
- Object storage: presigned reads and writes and multipart uploads for sandbox store, agent store and attachments; CommitSandBoxStoreManifest, StatSandBoxStoreObject, ListSandBoxStoreObjects. [SRC RPC]
- Multi-device: desktop registration per machine (SandMachine RPCs); presence reports; notifications on the active device only (gate). [SRC RPC, gate]
- Cloud pods and scale: coord tokens pod_hibernated, pod_proxy, pod_not_found_on_node, respawn_limit, cold_create, on_demand, cursorvm_failure. [SRC coord]
- Observability: OpenTelemetry traces (OTLP proto exporter); Sentry (electron and node-core); Statsig gates and dynamic configs; analytics TrackEvents, Batch, SubmitLogs; ClientNumericMetrics; gates sand_rpc_tracing, sand_process_metrics, sand_renderer_heap_metrics. [SRC]
- Local persistence: safeStorage (29 references across dist, mostly main-app and main-core), localStorage (10 references across bundles), node:sqlite (chrome-import-worker and others), data-root marker file .grokbot-data-root-v1 and files local-exec-daemon.json, host.lock, agents, gateway.json (names only). No IndexedDB or OPFS usage found. [SRC]
- End-to-end encryption or CRDT sync: not found (yjs has 6 name references, but no Y.Doc or provider usage found). [SRC absence, low confidence on the absence]

### Category 10. Governance, security and compliance
- Privacy: user privacy mode (GetUserPrivacyMode); team Privacy Mode strings; link to privacy settings. [SRC RPC, string]
- Admin controls: team policy chunks; "Awaiting admin approval"; Slack and Teams admin approval strings; 41 Admin* RPCs; AdminBreakGlassDeleteSandBoxPod (break-glass pod deletion); AdminRefreshSandBoxEgress; AdminGetSandBoxStoreStatus. [SRC RPC, string]
- Egress policy: GetGrokBotEgressPolicy; AdminRefreshSandBoxEgress; sandbox network policy in daemon. [SRC RPC, daemon]
- Approval and permission model: local tool permission (always_allow, needs_approval, blocked); wildcard allowlist strings; connector always-allow; auto-review approvals; ask_admin_role. [SRC]
- Secrets and vault: see category 7; secrets described as never shown to teammates; 1Password-backed secret path; credential request approval. [SRC]
- Passkeys and WebAuthn: StartPasskeyEnrollment, VerifyPasskeyEnrollmentCode, BeginPasskeyRegistration, FinishPasskeyRegistration, GetPasskeyCeremony, GetPasskeyCeremonyStatus, ListPasskeys, RevokePasskey; native signer dist/native/sand-webauthn-signer.exe; string that passkeys are needed to approve purchases; WebAuthn error codes SAND-E0211 (webauthnSignFailed) and SAND-E0212 (webauthnDesktopFailed) with domain auth. [SRC RPC, string, main-app]
- Chrome cookie import with per-origin approval: gate sand_import_chrome_cookies; hasChromeCookieImport environment flag; chrome-import-worker.cjs; cookie-origin-approval provider in coord; "Chrome cookie origin approval failed" strings. Behavior not executed or verified. [SRC]
- Sandbox enforcement: sandbox policy path for workspace_readonly and workspace_readwrite requires a native sandbox binary; error text says the binary is not configured and suggests insecure_none. No sandbox binary is shipped in dist (only sand-webauthn-signer.exe under dist/native). Windows behavior unverified. [SRC; medium]
- Audit and transcript retention: CommitGrokBotTranscriptEntries; AdminGetSandAgentTranscriptPage; IngestConversation, UploadIssueTrace, DownloadIssueTraces (analytics); no explicit audit-log or retention UI found (token OptionRetention only). [SRC; low]
- Export: diagram export and transcript strings; no data-export or eDiscovery feature found. [SRC absence, medium]
- SSO and SCIM: not found (only unrelated substring matches). Device-managed sign-in policy through env CURSOR_MDM_SIGN_IN_POLICY_JSON. [SRC]
- Data residency, DLP and moderation: not found. [SRC absence]
- Team scoping: codes team_required, team_policy_unavailable; Team Bot scope; one Teams organization per Bot. [SRC]
- Guest, external and shared-channel terms: zero English-candidate hits (only unrelated XML and colour-channel strings). Bot sharing uses access principals and link access (see above); no guest-role term found. [SRC absence, medium]

### Category 11. Client shell
- Electron 42.1.0 shell with 4 browser windows [REA]; WebContentsView use; webview guests with secure overrides (nodeIntegration disabled, contextIsolation on) [SRC main-app; REA reports 11 explicit web-preference sets].
- Custom protocols: sand:// scheme; grokbot:// deep link (MCP OAuth callback); default protocol client registration (setAsDefaultProtocolClient) and privileged scheme registration. [SRC]
- Auto-update: autoUpdater references (7); track "stable"; min client version dynamic config (sand_min_client_version); codes manual_update and auto_update. [SRC]
- Native dialogs and file access: showOpenDialog, shell.openExternal, stageAttachmentFile; file reads through the desktop bridge (main fs). [SRC]
- Keyboard and mouse: CmdOrCtrl+, (settings), CmdOrCtrl+R (reload), F11 (fullscreen), Shift+CmdOrCtrl+Z (redo); mouse side-button history navigation (sand:mouse-history-navigation); react-hotkeys-hook. [SRC]
- Window and layout: fullscreen title bar, scale-layer, scroll-pane, wipe-navigation transitions, sizeCapture, interaction-scope chunks. [SRC chunk names]
- Zoom: getZoomFactor exposed on the desktop bridge. [SRC]
- Tray and global shortcuts: no Tray or globalShortcut usage (0 references). [SRC absence]
- Electron screen capture: desktopCapturer and getDisplayMedia (0 references); screen views come from VNC and computer-use tools instead. [SRC absence]
- Microphone: getUserMedia (4 references) and microphone permission strings. [SRC]
- Internationalization: 35 locales in main catalogs and renderer strings; Lingui 5; emoji-messages chunks for 11 locales; language labels include English, French, German, Japanese, Korean, Chinese Simplified, Hindi, Arabic variants, Portuguese variants and others. [SRC]
- Themes and colors: appearance settings section; avatar color names (see category 7). Dark-mode behavior not verified. [SRC; low]
- Accessibility: spellcheck toggle ("Check Spelling While Typing"); keyboard settings section. Reduced motion and screen-reader support not verified. [SRC; low]
- Multi-account: sign-in codes account_required, account_generation_damaged, wrong_account_suspected; workspace switching UI not verified. [SRC; low]
- Onboarding: onboarding-flow chunk; terms-seen RPCs; onboarding copy such as "Meet a future teammate" and "Hand off any task to your team of Bots". [SRC]

### Category 12. UX patterns and layout
- Navigation model: "New chat" entry with a "Team Bots" section (string: a published Bot can be found under New chat, then Team Bots); sidebar with pinned Bots and sections. [SRC string, coord]
- Creation flows: new-agent-pane and new-chat-bar chunks; "New team bot", "New skill", "Add new secret" and template editor dialog (template-editor-dialog chunk). [SRC]
- Settings navigation (section-name reference counts in renderer): plugins 51, team 47, skills 40, secrets 36, general 29, automations 26, keyboard 25, account 14, connectors 10, notifications 4, appearance 4, security 3, billing 3, privacy 1. [SRC]
- Thread and transcript view: thread-card, transcript, settled-message-card and scroll-pane chunks; "Open threads beside the main chat" string. [SRC]
- Panels and dialogs: info pane, footer pill, async tasks panel, team-bot details pane, team-bot overview panel, network-debugger-dialog, computer-screen-preview, box-desktop-modal, fullscreen file viewer, approval-mode-switcher, auto-review-selector. [SRC chunk names]
- Empty states: "No unread Bots", "Nothing to mention yet", "Nothing installed yet" with a pointer to the marketplace. [SRC string]
- Density, information architecture details and onboarding flow content: not verified beyond chunk names. [INF, low]

### Developer-visible and configuration surfaces
- Environment variables (names only): SAND_PACKAGED, SAND_LAB, SAND_BACKEND_URL, SAND_HOST_GATEWAY_URL, SAND_CLIENT_APP_VERSION, SAND_SENTRY_RELEASE, SAND_SENTRY_ENVIRONMENT, SAND_RESTART_EXIT_CODE, SAND_WEBAUTHN_SIGNER_PATH, SAND_VOICE_HARNESS (values backend or package), SAND_SEND_POST_TIMEOUT_MS, SAND_ROSTER_READ_TIMEOUT_MS, SAND_DISABLE_SLIM_AVATARS, SAND_DISABLE_GATEWAY_STREAM_LIVENESS, SAND_DISABLE_GATEWAY_HEALTH_TTL, SAND_ATTACH_PROD_BOX; CURSOR_API_BASE_URL, CURSOR_WEBSITE_URL, CURSOR_MDM_SIGN_IN_POLICY_JSON; DEBUG; HTTPS_PROXY, HTTP_PROXY, NO_PROXY; CODEX_HOME and CLAUDE_CONFIG_DIR are referenced (purpose unverified). [SRC; INF on purpose, low]
- Statsig feature gates (checkFeatureGate): agent_prompted_cookie_sync, client_numeric_metrics, grok_bot_notify_active_device_only, grok_bot_store_uploads, sand_create_temporal_agents, sand_import_chrome_cookies, sand_mcp_apps, sand_messages_tools, sand_renderer_heap_metrics, sand_transcript_server_tail, sand_transcript_store_read, sand_xvf_typeface. Dynamic configs (getDynamicConfig): sand_internal_release_track_override, sand_min_client_version, sand_process_metrics, sand_rpc_tracing. [SRC]
- Command-line options and tool flags: --cursor-canvas-* layout flags (renderer); ripgrep-style search flags (--case-sensitive, --multiline, --after-context, --files, --iglob and others) in daemon bundles for an embedded search tool. [SRC]
- IPC channels (literal names): sand:mouse-history-navigation, sand:vnc-viewer-visible, sand:vnc-session, sand:vnc-liveness, sand:vnc-interactive, sand:vnc-host-key, sand:vnc-accepts-input, sand:renderer-boot-gate-opened, sand:renderer-boot-gate-landed, sand:renderer-alive, sand:render-trace-port, sand:network-online, sand:main-edge-served, sand:encode-png, sand:coordinator-port-request, sand:coordinator-port, sand:browser-popup-closed, sand:browser-passkey-stalled, sand:browser-origin-return, sand:boot-snapshot-sync, sand:avatar-source, sand:new-chat, sand:choose-main-agent. Bridges exposed to renderer: desktop (main, boot, environment, local), coordinatorPort, sandRenderTrace, desktopMount. Most other channels are minified constants (REA: 13 dynamic). [SRC; REA]
- Custom URI schemes: sand://; grokbot://; grokbot://mcp/oauth/callback. [SRC]
- Protocol services: aiserver.v1 GrokBotService (309 methods), DashboardService (73), SandBoxService (55), AgentStoreService (24), AiService (8), AnalyticsService (8), BackgroundComposerService (7). Also an outbox.v1 JSONL format. Full lists in scratchpad/tmp/svc_methods.json. [SRC]
- Embedded prompt and instruction text: present in the bundles (section headers such as the voice channel guidance); contents not reproduced. [SRC]
- Settings sections: plugins, team, skills, secrets, general, automations, keyboard, account, connectors, notifications, appearance, security, billing, privacy. [SRC]
- Routes and menus: renderer route literals /agents and /agent-data/ (SRC); Electron menu accelerators in main-app are the standard edit set plus CmdOrCtrl+, (settings), CmdOrCtrl+R (reload), CmdOrCtrl+A/C/V/X/Z, Shift+CmdOrCtrl+Z and F11; the colour, language and playback-speed label lists are settings or picker labels, not app menus (SRC).
## 4. Extension points

- MCP client: user-level and per-Bot MCP configuration; OAuth-backed MCP accounts; HTTP MCP status check; sandbox MCP tool execution (ExecuteSandMcpTool) and MCP resources (ReadSandMcpResource); MCP apps; daemon-level MCP servers (configured_servers, mcps_root). [SRC]
- Plugins: marketplace with the cursor-plugins manifest format (.cursor-plugin/marketplace.json referenced); per-Bot plugin variables; custom plugins; install, update and uninstall; team-level popularity. [SRC]
- Skills: SKILL.md format; managed skills (GetManagedSkills); per-user skills snapshot; teach-by-demonstration may create skills [INF, medium]. [SRC]
- Templates: exportable Bot templates with versions and a public marketplace listing; import details; create a Bot from a template. [SRC RPC]
- Webhooks (inbound): per-automation webhook API key (CreateAutomationWebhookApiKey) with a request-key pattern. [SRC]
- Email: per-Bot inbox addresses with send and read calls. [SRC RPC]
- Chat apps: Slack bot app and Microsoft Teams bot app (install, reinstall, uninstall, connect); Discord planned (string only). [SRC]
- Connectors: OAuth connector grants and always-allow rules. [SRC RPC]
- Native addons: cursor-proclist, sand-notification-settings, tree-sitter and tree-sitter-bash; WebAuthn signer helper executable. [SRC]
- Public API, SDK or CLI: none found in the bundle (no public OpenAPI, no SDK package, no CLI command set). The internal Connect RPC surface (aiserver.v1) is the only API layer and is used by the app itself. [SRC absence; INF that it is internal]

## 5. Data and protocol hints

- Wire: Connect RPC (unary and server-streaming, protobuf-es v1 descriptors in proto.cjs); JSON payloads validated with Zod; WebSocket (ws) and server-sent events. [SRC]
- Auth: OAuth 2.0 authorization code with PKCE (code_challenge: 4 references), refresh tokens (10 references), offline_access scope (1 reference); in-app OAuth popup handling (browser-popup-closed, browser-origin-return); MCP OAuth callback via deep link; WebAuthn and passkey ceremonies through the native signer; Slack and Teams OAuth installs; GitHub connect flow; 1Password connection and OAuth flows. Token values not recorded. [SRC]
- Local identity and daemon state: local-exec daemon credential file (path referenced in code; content not read); host.lock and local-exec-daemon.json in the data root; data-root marker .grokbot-data-root-v1. [SRC names only]
- Local storage: safeStorage encryption (29 references across dist, mostly main-app and main-core); localStorage (10 references across bundles); node:sqlite (DatabaseSync) in chrome-import-worker and main files (cookie database reads); outbox.v1.jsonl; no IndexedDB or OPFS. [SRC]
- Object storage: presigned URLs for reads and writes; multipart uploads with complete and abort; manifest commits for sandbox store; versioned agent store with file locks and tombstones. [SRC RPC]
- Realtime and sync: WebSocket, server-sent events, long polling, server-streaming RPCs, reverse channels with heartbeats and liveness windows, send journal with nonce and digest acceptance, resume windows, queued offline state. [SRC]
- Telemetry: OpenTelemetry OTLP traces; Sentry; Statsig gates and dynamic configs; analytics TrackEvents, Batch and SubmitLogs; IngestConversation (conversation upload name only; purpose not verified); UploadIssueTrace and DownloadIssueTraces. [SRC]
- Integrity: ELECTRONASAR integrity resource embedded in Grok Bot.exe [REA]; REA reports integrity contradictions for 5 native files (observed bytes untrusted) [REA].
- Opaque identifiers: a 32-hex identifier and telemetry keys exist in bundles; values not recorded. [SRC]

## 6. Hosts referenced (hostnames only)

- Platform and AI: cursor.com, www.cursor.com, api2.cursor.sh, api3.cursor.sh, api.origin.cursor.com, prod.authentication.cursor.sh, authenticator.cursor.sh, downloads.cursor.com, prodregistryv2.org and featureassets.org (purpose unverified); x.ai, api.x.ai, docs.x.ai, grok.com; statsigapi.net, api.statsigcdn.com, console.statsig.com, docs.statsig.com. [SRC]
- Integrations: slack.com, app.slack.com, docs.slack.dev; teams.microsoft.com; 1password.com, developer.1password.com, cache.agilebits.com; github.com, gist.github.com, raw.githubusercontent.com, gitlab.com, bitbucket.org, dev.azure.com; www.figma.com, linear.app, www.notion.so, www.canva.com, miro.com, coda.io, airtable.com, app.hex.tech, app.box.com, onedrive.live.com, 1drv.ms, drive.google.com, docs.google.com, calendar.google.com, mail.google.com, www.googleapis.com. [SRC]
- Library and reference links: sentry docs, react.dev, base-ui.com, tailwindcss.com, prosemirror.net, langium.org, mermaid and other library references. [SRC; not product behavior]

## 7. Blockers and limits

- REA: application and semantic graphs are partial (1,115 application unknowns; 13,215 semantic unknowns). 13 of 15 IPC operations are dynamic and unresolved. All 4 trace seeds returned no-match, so REA gave graph-level counts only. Integrity contradictions on 5 native files (bytes untrusted). Large REA views were saved to tool-results files and summarized with jq rather than read in full.
- REA resource-constraint limitations: 545 entries say that expanded application relationships exceeded the parent result budget (536 for renderer files; 9 for main, preload, daemon and coordinator files). Complete static facts were retained on the file nodes, so relationship-level coverage is partial for those files. [REA]
- Minified bundles: identifiers are mangled (for example $he, V$e, LYe). IPC channel constants could not be mapped to names; only literal sand: channels are named.
- English UI strings are split between inline JSX and hashed Lingui catalogs; string counts across the 35 locale folders inflate feature counts, so the English subset was used. Lingui catalogs were not read entry by entry. Absence claims based on strings are grep-level only (medium confidence).
- No runtime observation: the app was not launched (rule). Server-side behavior, quotas, plan gating and per-tenant feature availability are unknown. RPC names come from client descriptors only; message schemas were not expanded.
- Windows install: no sandbox binary is shipped under dist (only sand-webauthn-signer.exe in dist/native); Windows sandbox behavior is unverified. macOS-only features (Messages, Contacts helpers, macOS permission checks) cannot be confirmed active on Windows.
- Not analyzed: native .node files (no disassembly); Grok Bot.exe code (resource table only); sand-webauthn-signer.exe (no PE resources; strings not extracted); LICENSES.chromium.html (not read); renderer chunk bodies (594 files, 32 MB) were enumerated by name and grep hits, not read in full.
- Secrets: none printed or recorded. Telemetry keys and one opaque 32-hex identifier present in bundles were not recorded. A credential file path name referenced in code was not read. No credential, session, cookie or per-user stores were opened.
- Not run and not modified: no installers, apps, tests, builds or dev servers; no git write commands; nothing written outside the scratchpad.

## 8. Totals

- Feature candidates listed in section 3: 159 bullets (categories 1 to 12 plus developer-visible surfaces).
- Feature-gate names: 12 checkFeatureGate plus 4 getDynamicConfig names.
- RPC method entries: 309 + 73 + 55 + 24 + 8 + 8 + 7 = 484 across seven aiserver.v1 services; 429 distinct names; all 55 SandBoxService names also appear in GrokBotService. [SRC; scratchpad/tmp/svc_methods.json]
- Renderer chunks: 532 chunk-*.js; locales 35; integration brand logos 16.
- Declared dependencies (package.json): 94; no devDependencies or scripts. [SRC]
