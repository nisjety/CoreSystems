# Slack desktop 4.52.178 - inventory scout (T1)

Scope: read-only audit of the pre-extracted copy scratchpad/extract/slack/ (package.json, dist/, node_modules/) plus a read-only listing of the installed resources/ folder. Nothing was launched, no project code was run, nothing was written outside scratch. Dynamic behaviour is NOT verified.

Shorthand (paths under .../scratchpad/extract/slack/): pkg = package.json; D = dist/; NM = node_modules/; M = dist/main.bundle.cjs; P = dist/preload.bundle.js; C = dist/component-preload-entry-point.bundle.js.
Pointers "@N" are byte offsets from grep -a -b -o on the named file (bundles are minified: M has 84 lines, so byte offsets are the usable pointer).
Tags: [REA] = REA analysis of shipped artifact; [SRC] = read directly in files; [DOC] = public docs (URLs given; most were read through search-result summaries, not full pages); [INF] = inference (basis stated). Confidence H/M/L where it matters.

## 0. REA usage log

- mcp__rea__analyze_javascript_application: ONE run, format=directory on the extracted folder, detail=summary, integrity_policy=record-and-continue, max_heap_mb=2048, analysis_timeout_ms=600000. Coverage: application and semantic = partial (budget-limited). Stats: 662 relevant files, 454 parsed JS files, 1612 findings, 0 parse failures, 0 nested asar containers. Electron block: browser_windows 0; context_bridge_apis 1; ipc operations 3, all DYNAMIC channel expressions (literal channels 0); renderer transmissions 2; renderer listeners 1; main handlers 0; sender-validation observations 4; utility processes 0; native addon bindings 0. Parent analysis evidence id (session-scoped): ev_05c9f0e1dfaf0c7a27858a5136a7f42bd98da55e8da0938a470ad14b7bfe2552.
- mcp__rea__inspect_analysis_view: summary (read from the saved result) and the modules page 0-30 of 648. The sections view is rejected for JS evidence (binary-layout only).
- mcp__rea__trace_application_feature: seed string "slack://channel" (contains) and seed channel "redux-action" (exact). Both returned no-match on a partial graph. REA therefore did NOT see the deep-link or IPC names that grep found. Treat REA's IPC view as incomplete.
- Not used (not applicable): inspect_pe_resources, search_strings, list_strings, binary_overview, inspect_artifact. No native executables were analysed and the .node addons were not opened.
- REA tool-result JSON files were read only with jq for header fields. These are this session's own tool outputs, not other session data.
- Everything marked [SRC] below comes from grep and read of the bundles and package files, not from REA.

## 1. Identity, tech stack and dependencies

- Product: Slack desktop, an Electron shell around the remote slack.com web client [SRC pkg; DOC]. Package name slack-desktop, productName Slack, version 4.52.178, main ./dist/boot.bundle.cjs, engines node >= 24 [SRC pkg].
- desktopMinimumSupportedVersion 4.37.0; supportedOS darwin >= 10.10; packageManager yarn@4.12.0; yarn workspaces spec/e2e, spec/old-joe, tools/triage-agent; linuxPackageInfo genericName "Slack Client for Linux" [SRC pkg].
- Electron: devDependency electron@44.4.5 [SRC pkg]; runtime assumed identical [INF]. A Chromium license bundle ships in the install (LICENSES.chromium.html, about 20 MB) [SRC install resources].
- Language: minified JavaScript only (CJS main bundle, browser bundles). No TypeScript source and no .map files shipped, although bundles carry sourceMappingURL comments [SRC D].
- Stack from devDependencies and bundle identifiers:
  - Bundling: @rspack/core 2.1.3, @swc/core 1.11.31, webpack-merge, style-loader, less 4.1.1 [SRC pkg].
  - State and async: redux, redux-observable (epics), redux-persist with redux-persist-node-storage, rxjs, rxjs-spy, rx-sandbox. RxJS identifiers are present in the shipped bundles [SRC pkg; SRC M, P].
  - UI tooling: preact, react-redux types, react-transition-group, react-resize-detector [SRC pkg].
  - Telemetry: @sentry/webpack-plugin 5.4.0; Sentry identifiers and sentry.io hosts in the bundles [SRC pkg; SRC D].
  - Packaging and signing: Electron Forge 8.0.0-alpha.8 (maker-dmg, maker-zip, maker-base), electron-winstaller 2.5.1 (Squirrel), electron-windows-msix 1.1.7, electron-wix-msi 3.2.0, @electron/osx-sign 2.4 and @electron/notarize 3.1, @electron/fuses 2.1 (Electron fuse hardening), @electron/asar 4.2, @erickzhao/electron-versions 4.1.0 [SRC pkg].
  - Quality and tests: jest 30.4.2 with jsdom, jest-extended, fetch-mock and junit reporter; @testing-library/dom, preact and user-event; husky, lint-staged, oxlint with oxlint-tsgolint, knip 6.31.0, dependency-cruiser, prettier, typescript and @typescript/native-preview [SRC pkg].
  - i18n and release tooling: @tinyspeck/i18n-translation-tools 1.0.5, message-format, google-spreadsheet 4.1.4, google-auth-library 9.14.0, @octokit/rest 22, @aws-sdk/client-s3, @slack/web-api 7.13.0 (devDep), canvaskit-wasm 0.39.1 (use not determined). Purposes are INF [SRC pkg].
  - Proxy and network dev tooling: http-proxy, proxy-agent, set-cookie-parser, node-fetch, url-parse, winston (logging), joi (validation) [SRC pkg].
- Runtime dependencies (8 in pkg dependencies), grouped by capability:
  - Native keyboard: @tinyspeck/native-keymap 2.2.3 ("Get OS key mapping"). Its header attributes the code to Microsoft under MIT [SRC NM].
  - Native desktop extensions: @tinyspeck/slack-desktop-utils 1.27.0 ("Native extensions for the Slack desktop client", UNLICENSED, node-pre-gyp binding, Windows and macOS branches). Its exports include accelerator-combo helpers such as validateAcceleratorComboCallbacks and normalizeAcceleratorComboKeys [SRC NM].
  - Notification and focus state: macos-notification-state 2.0.2 (macOS Do Not Disturb, native addon) and windows-focus-assist 1.4.0 (Windows Focus Assist status) [SRC NM].
  - Managed configuration (macOS): cf-prefs 2.0.1 ("access managed app preferences on macOS") [SRC NM].
  - OS integration: registry-js 1.16.1 (installed; pkg asks for ^1.15.1; Windows registry access) and file-handler-info 0.2.1 (native file_handler_info.node; purpose inferred as OS default-app lookup) [SRC NM; INF].
  - Auth helper: electron-native-auth 0.1.1 (native electron_native_auth binding, no description in the package). Purpose unknown [SRC NM; confidence L].

## 2. Directory and module map

Top level of extract/slack: pkg, LICENSE (0 bytes), D/, NM/ [SRC].

- pkg - manifest: scripts, dependencies, build configuration, workspaces.
- D/ - all shipped bundles, HTML shells, fonts, icons and sounds, flat (no source tree) [SRC].
  - boot.bundle.cjs (61 KB) - Electron main entry; requires ./main.bundle.cjs [SRC].
  - main.bundle.cjs (1.7 MB, 84 lines) - main process: Redux store and epics, windows, menus, tray, global shortcuts, updater, deep links, IPC, telemetry [SRC].
  - 898.bundle.cjs (172 KB) and 863.bundle.cjs (5 KB) - shared chunks of the main-process build [SRC].
  - preload.bundle.js (764 KB) - preload for the web-app window; exposes slackPlatform (P@762422) [SRC].
  - component-preload-entry-point.bundle.js (618 KB) - preload for desktop component windows; exposes the desktop bridge (C@618065) [SRC].
  - child-preload-entry-point.bundle.js (16 KB) - preload for child windows [SRC].
  - magic-login-preload-entry-point.bundle.js (697 B) - preload for the magic-login flow; IPC response name SLACK_MAGIC_LOGIN_RESPONSE [SRC].
  - call-get-macos-notification-state-entry-point.bundle.cjs, call-get-media-access-status-entry-point.bundle.cjs, call-get-windows-focus-assist-entry-point.bundle.cjs, call-get-windows-notifier-state-entry-point.bundle.cjs - tiny helpers run in a worker thread or utilityProcess to query OS state (see 3.5) [SRC].
  - notifications.bundle.js (325 KB) with notifications.html - HTML notification window UI [SRC].
  - settings-editor.bundle.js (308 KB) with settings-editor.html - settings editor window (table of settings rows with computed values) [SRC].
  - about-box.bundle.js (314 KB) with about-box.html - About window [SRC].
  - net-log-window.bundle.js (306 KB) with net-log-window.html - net-log capture window [SRC].
  - basic-auth-view.bundle.js (134 KB) with basic-auth-view.html - HTTP basic-auth prompt for proxies and sites [SRC].
  - *.mp3 (23 files) - sound pack: 9 call sounds (calls_*_v2.mp3) and about 14 notification or UI sounds (flitterbug, hummus, boop, short_boop, b2, been_tree, animal_stick, here_you_go_lighter, hi_flowers_hit, item_pickup, knock_brush, confirm_delivery, save_and_checkout, complete_quest_requirement) [SRC].
  - *.woff2 (9 files) - slack-icons font and the Lato family [SRC].
  - Hash@1x/2x/3x.png and net_logging_enabled/stopped@2x.png - images for the net-log UI [SRC].
  - resources/ - macos-menubar-icon-Template{,@2x,@3x}.png (menu-bar icon); slack-taskbar-{rest,unread,highlight}.ico (Windows taskbar states); locales/{de,en,es,fr,it,ja,ko,pt,zh}/desktop_{browser,external,general,menu,messagebox,renderer}_en_GB.json (9 languages x 6 namespaces); unreads-badge/unread-badge-resources.json [SRC].
- NM/ - 72 top-level entries, including 2 scope folders (@isaacs, @tinyspeck). Holds the 8 runtime deps plus the node-pre-gyp, prebuild-install and tar stack used to load .node binaries [SRC].
- Absent: src/ tree, renderer UI source (the chat UI is remote), Electron binary, .map files, and code for huddles, canvases, lists, workflows, clips [SRC].

Install folder (read-only listing): C:/Program Files/WindowsApps/com.tinyspeck.slackdesktop_4.52.178.0_x64__8yrtsj140pw4g/app/resources/ holds LICENSES.chromium.html, SlackAppList.png, SlackLogo.png, SlackSmallTile.png and VisualElementsManifest.xml (Start-menu tile manifest) [SRC]. The extracted copy was used for analysis; the install was not analysed further.

## 3. Feature candidates (grouped by taxonomy; bullet = one capability)

Format: capability - [tag] pointer. Confidence where it matters.

### 3.1 Messaging core (taxonomy 1)

- Message and link context menu: Copy Link, Open Link, Copy Image, Copy Image URL, Copy Email Address, Search with Google, Paste and Match Style, Inspect Element - [SRC] desktop_menu_en_GB.json; contextMenuEpic and CONTEXT_MENU_CLICKED in M. H.
- Clipboard bridge: WRITE_STRING_TO_CLIPBOARD IPC; clipboard.writeText, readText, clear - [SRC] M. H.
- Spellcheck with downloadable dictionaries: setSpellCheckerLanguages (3), dictionary hosts spellcheck.slack-edge.com, .mil and -gov variants, Add to Dictionary menu, dictionaryEventsEpic at M@944650 - [SRC]. H.
- File-share confirmation dialog: "You're about to share the file '{fileName}' with others in the channel '{channelId}'" - [SRC] desktop_messagebox_en_GB.json key 46843414. The code that shows it was not located. M.
- Navigation and external-link control: will-navigate (5), will-redirect (1), setWindowOpenHandler (1), shell.openPath and shell.showItemInFolder - [SRC] M. H.
- Presence and idle detection: systemIdleStatusEpic at M@945836, SLEEP_AND_WAKE IPC, powerMonitor.getSystemIdleTime (1), isMachineAwake (2), onBatteryPower (2) - [SRC]. H on existence; the presence semantics are INF.
- Unread state carried to the OS: slack-taskbar-unread.ico, unreads-badge JSON, setBadge (4), setOverlayIcon (2) - [SRC]. H (see 3.5).
- Offline and reconnect handling: dialog strings "Slack is having trouble loading" and the slow-network text; GET_NETWORK_INFO IPC; app.networkStatus (2); TICKLE_MESSAGE_SERVER action at M - [SRC]. H on existence; mechanism INF.
- Rich text, emoji, reactions, threads, polls, drafts, pins, saved items, bookmarks, forwarding, scheduled send, voice clips, read receipts, typing: NOT in the shell (case-sensitive hits: Emoji 0, Reactions 0, Poll 0, Thread 1; lowercase counts are library noise such as immer "drafts"). These features live in the remote web client - [SRC absence] H; location INF.
- Clips (capture an update and pass it on) - [DOC] slack.com navigation, search summary only. M.
- Scheduled messages and reminders in the shell - [SRC absence]: Reminder 0; "scheduled" hits are Node stream code. M.

### 3.2 Org and membership (taxonomy 2)

- Multi-workspace switcher: team-list rail (token team-list-end-separator at M@787028), menuitem-select-next-team and menuitem-select-prev-team, team list in the tray state - [SRC]. H.
- Workspace keyboard shortcuts Ctrl/Cmd+1..9: accelerator template "CmdOrCtrl+${t}" for t <= 9 in M - [SRC]. H.
- Sign in to another workspace: menu item "&Sign in to Another Workspace..." (desktop_menu) and "Workspaces" label (desktop_general) - [SRC]. H.
- Workspace identity in payloads: team_id, team_name, team_url; WORKSPACE_SELECTION IPC at M - [SRC]. H.
- Sign-out and reset: "Reset App Data..." menu and the "Reset Slack?" dialog, which says it signs out of all workspaces and resets the app (desktop_messagebox) - [SRC]. H.
- SSO, native direct-SAML mode: PLATFORM_NATIVE_DIRECT_SAML at M@789602; code paths /ssb/signin and /sso/saml/start?redir= - [SRC]. H. [DOC] Setup of SAML SSO (slack.com/help/articles/203772216): owners keep password login if the IdP fails, a 72-hour linking window, signed SAML responses - search summary. M.
- Magic login: magic-login preload plus SLACK_MAGIC_LOGIN_RESPONSE; the preload returns the page innerText over IPC - [SRC]. M (purpose INF: magic-link sign-in).
- Regional and restricted deployments: domain families slack.com, slack.eu, slack-gov.com, slack.mil and -dev variants for app, edge, imgs, files and msgs (M and P strings); enum values RESTRICTED and MILITARY (M, function du) - [SRC]. H on existence; [DOC] data residency (slack.com/help/articles/360035633934) and GovSlack (api.slack.com/govslack), search summaries. M.
- Feature-flag switches tied to account state: switchClientFeatureViaSignOut, appActionsViaDeepLinks, sign_in_by_default - [SRC] M (meaning INF).
- Admin console, roles, guests, external users, Slack Connect, Enterprise Grid, SCIM, user groups: NOT in the shell (case-sensitive: Admin 0, Enterprise 0, Guest 0, Grid 0; SSO 22 hits, not inspected in full) - [SRC absence] H. [DOC] Slack Connect guidelines (slack.com/help/articles/4408191650451), SCIM provisioning (slack.com/help/articles/212572638), Enterprise Grid owners - search summaries. M.
- Dev environment selection: label "Production" (desktop_general), deep link slack://open?devEnv=..., dev hosts dev.slack.com and dev.slackb.com - [SRC]. M.

### 3.3 Calls and meetings (desktop plumbing only; the call UI is remote) (taxonomy 3)

- Call window teardown: CLOSE_CALL and CLOSING_CHILD_WINDOWS IPC names at M@961376 - [SRC]. M.
- Call sound pack: calls_incoming_ring_v2, calls_outgoing_ring_v2, calls_alert_v2, calls_confirmation_v2, calls_pop_v2, calls_they_joined_call_v2, calls_they_left_call_v2, calls_you_joined_call_v2, calls_you_left_call_v2 (D/*.mp3). The playback code was not located - [SRC]. H on existence.
- Microphone and camera permissions: getMediaAccessStatus (9), askForMediaAccess (1), utility service slack-media-access-check, call-get-media-access-status-entry-point, desktop_media_access traces (cached, fresh, force_refresh) - [SRC]. H.
- Audio device checks: desktop_can_init_audio_device and desktop_get_audio_device_details traces - [SRC]. H.
- Screen share capture: desktopCapturer.getSources (1); the string "screenshare" appears 40 times, mostly in web bridge code. No setDisplayMediaRequestHandler found - [SRC]. M.
- Global mute hotkey: toggle_mute bound to CommandOrControl+Shift+Space (M@995144); the trigger goes to the renderer as NOTIFY_GLOBAL_SHORTCUTS_TRIGGER - [SRC]. H. [DOC] Slack keyboard-shortcut article (slack.com/help/articles/201374536) lists Ctrl+Shift+Space for muting in a huddle on Windows and Linux and Cmd+Shift+Space on Mac - search summary. H.
- Huddle start, join, leave or end shortcut Ctrl+Shift+H - [DOC] search summary only. The shell does not register it - [SRC absence]. M.
- Sleep prevention while media is active: powerSaveBlocker (11 mentions), POWER_SAVE_BLOCKER_START IPC at M@1418069 - [SRC]. H on existence; the call-time purpose is INF.
- Recording indicator in the tray: a selector returns true when any state object has isRecording === true (M) - [SRC]. M. Which feature sets it is not determined; Clips or huddle recording are candidates - [INF] L.
- Huddles, transcripts, captions, huddle notes, meeting chat, calendar, whiteboards - [DOC] Huddles are described as audio and video calls (slack.com navigation, search summary). No local code: Huddle 0, Transcript 0, Whiteboard 0, Calendar 0 - [SRC absence] H.

### 3.4 Search and discovery (taxonomy 4)

- In-app search trigger: START_SEARCH IPC in M and C - [SRC]. M.
- Local file search: LOCAL_FILE_SEARCH = desktop_local_file_search constant and desktop_system_diagnostics; gating unknown - [SRC]. L.
- Spotlight handoff: spotlightHandoff epic at M@945562; the handler body looks empty - [SRC]. L (purpose unknown).
- Search with Google from the context menu - [SRC]. H (see 3.1).
- Search index and AI search answers - [DOC] slack.com navigation, search summary. Not in the shell. M.
- eDiscovery and DLP through the Discovery API - [DOC] slack.com/help/articles/360002079527: Enterprise Grid only, approved partner apps, JSON output. M.
- Quick switcher on Ctrl+K - [DOC] third-party guide only, not verified in the shell. L.

### 3.5 Notifications and attention (taxonomy 5)

- Delivery method switch: notificationMethod validator values html | winrt | window | null (M); notificationPlayback set to "web" (M); desktop_enforced_native_notifications trace at M@990362 - [SRC]. H.
- HTML notification window: notifications.html with notifications.bundle.js (325 KB), windowType NOTIFICATIONS - [SRC]. H.
- Force native notifications flag: FORCE_NATIVE_NOTIFICATIONS at M@796108 - [SRC]. H.
- Notification sound library: 14 named sounds in D (flitterbug, hummus, boop, short_boop, b2, been_tree, animal_stick, here_you_go_lighter, hi_flowers_hit, item_pickup, knock_brush, confirm_delivery, save_and_checkout, complete_quest_requirement) - [SRC]. H. The name complete_quest_requirement hints at a quest or onboarding feature - [INF] L.
- Focus and Do Not Disturb awareness: FOCUS_SESSIONS flag; on browser-window-focus an async DND check runs (aX, MONITOR_FOCUS_SESSION, ASYNC_DND_CHECK) - [SRC]. H on existence.
  - macOS source: macos-notification-state; its helper call-get-macos-notification-state-entry-point always posts value false in this build, so it looks stubbed - [SRC]. M.
  - Windows source: windows-focus-assist with per-app priority by AUMID (call-get-windows-focus-assist-entry-point) - [SRC]. H.
  - Windows notifier state: slack-desktop-utils getNotifierState (call-get-windows-notifier-state-entry-point) - [SRC]. H.
- Badges and attention cues: setBadge (4), setOverlayIcon (2), flashFrame (4), setProgressBar (2), window-flash IPC name, taskbar icons rest, unread and highlight - [SRC]. H.
- macOS notification re-authorisation: desktop_mac_notification_reauthorize and its skipped variant traces - [SRC]. M.
- Notification-state failure telemetry: desktop_notifier_state_failure trace and slack-notifier-state-check service - [SRC]. H.
- Notification warnings resource: notification-warnings.json (name only; content not read) - [SRC]. L.
- Digest, inbox, activity feed, snooze, per-channel mute, mention routing: not located in the shell (case-sensitive: Snooze 0, Digest 0, Mention 0; lowercase digest hits are crypto digest() calls). The only mute-related shortcut identified in the shell is toggle_mute - [SRC absence] M. Notification preference UI is in the web client [INF].

### 3.6 Apps and extensibility (taxonomy 6)

- Deep links registered for the slack:// scheme: slack://channel?team=... (open channel; the id parameter is not verified), slack://open?team=... (open workspace), slack://app/?action=reset-window, slack://noop, slack://open?devEnv=... (dev), slack://toolbelt-callback - [SRC] M. H.
- Deep-link forwarding into the renderer: FORWARD_DEEP_LINK IPC and the appActionsViaDeepLinks flag - [SRC] M. H.
- Protocol registration and single instance: setAsDefaultProtocolClient (2) at M@1426975; requestSingleInstanceLock at M@1437294 - [SRC]. H.
- Custom menu items added from the web app: CUSTOM_MENU_ITEM_CLICKED, externalMenuItemClickedEpic, POPUP_APP_MENU, app.customMenuOpened (7), feature flag feature_desktop_extend_app_menu - [SRC]. M.
- Toolbelt developer sign-in: OAuth against toolbelt.tinyspeck.com (/oauth/authorize and /oauth/token) with the slack://toolbelt-callback return; tokens persist under the app userData folder (M@832269). Token contents were not read; the file is secret-bearing - [SRC]. M (internal, developer-facing).
- Developer-facing shell features: Developer menu, Enable main process inspector, Show tracing UI, Toggle DevTools (CmdOrCtrl+Alt+I), Use local assets (useLocalAssetsEpic at M@948609) - [SRC] desktop_menu and M. H.
- Public Slack platform, used by partners and not shipped in this bundle: Web API (@slack/web-api is a devDependency), Events API, Socket Mode over WebSocket, Block Kit, Bolt SDKs (JavaScript, Python, Java), Slack CLI with `slack run` and live reload (April 2026 changelog), AI apps and agents, Slack MCP server - [DOC] docs.slack.dev/apis/events-api/using-socket-mode/, docs.slack.dev/changelog/2026/04/10/slack-cli, docs.slack.dev/ai, docs.slack.dev/ai/slack-mcp-server (search summaries). M.
- Workflow Builder, Lists, Canvas and Clips as user-built automation or artifacts - [DOC] slack.com navigation (search summary). Not in the shell: Workflow 0, Lists 0, Canvases 0, Clips 0 (case-sensitive); the only Canvas-related code is the handoff epic named in 3.8 - [SRC absence]. M.
- App directory and Marketplace - [DOC] mentioned in the GovSlack and eDiscovery summaries; no shell code. L.

### 3.7 Agents in the room (taxonomy 7)

- Slackbot Mini tray window: tray click opens the Slackbot Mini window; a right-click handler sits beside it (handleSlackbotMiniClick at M@1210075, log text "Controlling Slackbot Mini window via click") - [SRC]. H on existence.
- Slackbot hosts in the shell: "Slackbot" appears 3 times; hosts slackb.com, slackb.mil, slackb-gov.com, slackb-mil-dev.com, slackb-gov-dev.com and dev.slackb.com - [SRC]. M. Purpose is INF (Slackbot assistant surface).
- AI apps and assistant threads (in the remote platform, documented): agents as self-directed apps, side-by-side panel, app threads, progressive text, starter prompts, the "Agents & AI Apps" setting, Bolt Assistant class, assistant_thread_started and message.im events, assistant.threads.setStatus - [DOC] docs.slack.dev/ai/developing-ai-apps and docs.slack.dev/ai (search summaries). M.
- Agent design rules: people must always know when they are talking to automation; an agent's data reach stops at what the requesting person can already see - [DOC] docs.slack.dev/ai/agent-design (search summary). M.
- Slack MCP server: search across messages, files, people and channels; read and send messages; canvases and users; each client needs workspace approval and admin management; Slackbot MCP Client calls remote MCP servers - [DOC] docs.slack.dev/ai/slack-mcp-server (search summary). M.
- Slackbot as a personal AI assistant, Agentforce and "Slack Atlas" appear as product names in slack.com navigation - [DOC] search summary. Atlas is unverified. L-M.
- Agent runtime inside the shell (sandbox, browser or PC control, cron, memory, approvals, cost limits): NOT present. The sandbox flag in BrowserWindow webPreferences is renderer hardening, not an agent sandbox - [SRC absence]. M.
- Dev-side triage tool: workspace tools/triage-agent and script "triage" in pkg - [SRC]. L (contents not read).

### 3.8 Collaborative artifacts and files (taxonomy 8)

- File viewer window: windowType file_view (2) - [SRC]. H.
- Open downloaded files and external apps: desktop_open_downloaded_file trace, shell.openPath (2); messages "Slack can't open that kind of file", "This is a {fileExt} file and will open in {appName}... trust this file", "Always open {fileExt} files." - [SRC] desktop_messagebox_en_GB.json. H.
- Reveal in folder and log export: "Show Logs in Explorer", "Show Logs in File Manager", shell.showItemInFolder (2); log export message "This will save a log file to Slack's download location" - [SRC]. H.
- Canvas handoff: canvasHandoffEpic at M@945603; handler body not read (treat as stub until checked) - [SRC]. L.
- "Tasks" label in the general locale (key 2228063086) - [SRC]. L (purpose unknown).
- Security-scoped file bookmarks for macOS (manageBookmarksEpic at M@944608; process.mas branch) - [SRC]. M.
- Canvases, Lists, Clips, code and projects, review and approval flows: [DOC] only. Not in the shell - [SRC absence]. M.

### 3.9 Realtime and sync architecture (taxonomy 9)

- Main-to-renderer state sync: Redux actions on the IPC channel "redux-action" (M@222740; ipcMain.on, ipcRenderer.on and ipcRenderer.send all use it, also in P and C) - [SRC]. H on the channel, M on semantics. REA recovered no literal channel (see section 0).
- Main-process epics (redux-observable) for desktop features: showReleaseNotesEpic (M@945644), webContentsTimeoutDialogEpic (M@945784), systemIdleStatusEpic (M@945836), canvasHandoffEpic (M@945603), spotlightHandoff (M@945562), dictionaryEventsEpic (M@944650), manageBookmarksEpic (M@944608), useLocalAssetsEpic (M@948609), showNetLogWindowEpic (M@948846), showWebappDialogEpic, showAuthDialogEpic - [SRC]. H.
- Persisted client state: slack-persistor name; redux-persist with redux-persist-node-storage (devDependency); desktop_persistence_notification_failure trace - [SRC]. M (storage under the app userData folder is INF).
- Service-worker control: CLEAR_SERVICE_WORKER action and serviceWorkers reset - [SRC]. M.
- Message-server keep-alive: TICKLE_MESSAGE_SERVER action with meta omitFromLog (M) - [SRC]. M (semantics INF).
- Sign-in state machine: SIGN_IN_PROGRESS and SIGN_IN_COMPLETE actions - [SRC]. M.
- Speed-limit change: SPEED_LIMIT_CHANGE (M@961832) pushed to the renderer when the value crosses a 10-unit bucket; meaning not determined - [SRC]. L.
- Power and thermal: SLEEP_AND_WAKE IPC; powerMonitor.on (11); onBatteryPower (2); getCurrentThermalState (2) - [SRC]. H.
- Network information: GET_NETWORK_INFO and GET_NETWORK_INFO_RESPONSE IPC; net.request (3); net.log (2); netLogStatus - [SRC]. H on existence.
- Session metrics: session.id, session.started, session.lastActivity, session.segmentId (8), session.sampled (6); events session:start, session:end, session:duration - [SRC]. M.
- Rate limiter for one request type: session_limit and interval reasons, a 10-minute window and a count of 3 or more - [SRC]. L (purpose not determined).
- Secret encryption at rest: safeStorage.encryptString (6), decryptString (6), isEncryptionAvailable (9), getSelectedStorageBackend (2) at M@864113 - [SRC]. H on existence.
- Chromium switches set by the shell: enable-features, disable-features, enable-low-end-device-mode, disable-gpu, disable-renderer-backgrounding, disable-pinch, disable-accelerated-video-decode; Wayland detection via XDG_SESSION_TYPE and the ozone-platform wayland argument (P) - [SRC]. H.
- End-to-end encryption: no E2EE code found in the shell - [SRC absence]. M.

### 3.10 Governance, security and compliance (taxonomy 10)

- Electron hardening: contextIsolation true, sandbox true, nodeIntegration false, webviewTag false (M, C); @electron/fuses devDependency - [SRC]. H.
- Launch integrity check: desktop_integrity_check_failed trace, crash_integrity_check constant, integrity.failed flag, comparison of modifiedDateAtLaunch (M@1175614) - [SRC]. H on existence; the exact checks were not read.
- IPC sender validation: REA counted 4 sender-validation observations - [REA]. M.
- Permission gating: setPermissionRequestHandler (2), setPermissionCheckHandler (2) - [SRC]. H.
- Managed preferences on macOS: cf-prefs reads managed app preferences; notification enforcement looks policy-driven (desktop_enforced_native_notifications) - [SRC]. M (policy purpose INF).
- Crash and error telemetry: Sentry hosts sentry.io and browser.sentry-cdn.com; __SENTRY_IPC__ bridge (2); crashReporter.start (2) and addExtraParameter (2); @sentry/webpack-plugin devDependency - [SRC]. H.
- Health trace events: desktop_proxy_info, desktop_certificate_error, desktop_render_process_gone, desktop_unresponsive_*, desktop_feature_flags_loaded, desktop_performance_record, desktop_system_diagnostics, desktop_client_diagnostics - [SRC]. H.
- Proxy handling: desktop_proxy_info trace (proxy_enabled and value), basic-auth-view prompt, proxy dev tooling in pkg - [SRC]. H.
- Sign out everywhere: "Reset App Data..." menu item with a confirmation dialog titled Reset Slack - [SRC]. H.
- Dangerous-file warnings: "This is a {fileExt} file" text with a request to trust the file first - [SRC] desktop_messagebox. H.
- Multi-tenant session partitions: session.fromPartition (3), session.defaultSession (4) - [SRC]. M.
- Region and government host families (slack.eu, slack-gov.com, slack.mil and -dev): see 3.2 - [SRC]. H on existence.
- Data residency: [DOC] slack.com/help/articles/360035633934 - the region is chosen per org; messages, canvases, files and search index stay in region; profiles and some logs may sit elsewhere (search summary). M.
- Discovery API, eDiscovery, DLP, legal holds: [DOC] slack.com/help/articles/360002079527 - Enterprise Grid only; partner apps; JSON output (search summary). M.
- SAML and SCIM: [DOC] slack.com/help/articles/203772216 and slack.com/help/articles/212572638 (search summaries). M.
- GovSlack: [DOC] api.slack.com/govslack - US government eligibility; FedRAMP High and IL4 listed on the product page (search summary). M.
- Retention controls: [DOC] Salesforce Trailhead module on data retention (search summary). L.
- Session duration and idle timeout: [DOC] SAML help notes that session lengths may need setting inside Slack (search summary). L. No session-timeout timer was found in the shell.

### 3.11 Client shell (taxonomy 11)

- Window styling: macOS titleBarStyle hidden or frameless with trafficLightPosition x 14 and vibrancy; Windows backgroundMaterial; autoHideMenuBar; show false at creation; skipTaskbar - [SRC] M. H.
- Windows custom chrome: window-chrome and window-chrome-app-menu identifiers - [SRC]. M.
- Application menu (Menu.buildFromTemplate, 3 uses): File, Edit, View, History, Conversation, Window, Help, Developer - [SRC] desktop_menu_en_GB.json. H.
- Global hotkeys: toggle_mute (CommandOrControl+Shift+Space) and desktop_command_center (CommandOrControl+Shift+K) registered as global shortcuts; double-press modifier accelerators (command, option, control, shift, win) with a threshold; macOS accessibility check; combo register and unregister through slack-desktop-utils (M@1388759) - [SRC]. H.
- Menu accelerators: CmdOrCtrl+W, +Q, +R, +Shift+R, +Shift+V (paste and match style), +Alt+I (DevTools), Alt+Left and Alt+Right (back and forward), Alt+E/F/V/H/G/C (likely menu mnemonics, INF), F6 and Shift+F6 (section navigation), F11 (full screen), CmdOrCtrl+0, Plus and minus (zoom), edit-role keys - [SRC]. H.
- Zoom: setZoomLevel (3), ZOOM_IN IPC, webFrame.getZoomFactor (3) - [SRC]. H.
- Multi-display: screen.getAllDisplays (5), getDisplayMatching (4), getDisplayNearestPoint (3), healDisplayScaleEpic, Move Slack to current display menu item - [SRC]. H.
- Tray and menu-bar icon: macOS template PNGs at 1x, 2x and 3x; Tray with click and right-click handlers, tooltip (2), context menu (1); tray state carries teams, updateStatus, launchEnforced, locale and isRecording - [SRC]. H.
- Dock (macOS): app.dock (2) and setBadge - [SRC]. H.
- Windows identity: setAppUserModelId (1), VisualElementsManifest.xml, slack.exe, update.exe (Squirrel), slack_SquirrelSetup.log - [SRC]. H.
- Launch at login and background: setLoginItemSettings (5), openAtLogin (5), getLoginItemSettings (3); menus Keep in Background, Show Slack Immediately, Always Show Menu Bar; launchEnforced in tray state - [SRC]. H.
- Auto-update: autoUpdater with checkForUpdates (5), quitAndInstall (7), setFeedURL (1) and events update-available, update-downloaded, update-not-available; updater_variant query value msix or squirrel with an arm64 flag; desktop_updater_* traces; Keep Slack up to date dialog; Restart to Apply Update menu; Linux packagecloud script - [SRC]. H.
- Update-permission failure message: Slack lacks the permissions to update itself (desktop_messagebox locale text, paraphrased) - [SRC]. H.
- macOS installer helper: Move Slack to Applications dialog and running-from-installer text - [SRC]. H.
- Restart and reset tools: Clear Cache and Restart, Clear cache and exit, Restart and collect net logs, Restart and collect verbose debug logs, Enable or Disable hardware acceleration and restart, Restart Slack - [SRC] desktop_menu. H.
- Hardware acceleration: useHwAcceleration (5), disableHardwareAcceleration (1) - [SRC]. H.
- Crash and hang handling: webContentsTimeoutDialogEpic; unresponsive dialog with reload, wait and restart choices; crash dialog; desktop_render_process_gone; extended_unresponsive_timeout constant - [SRC]. H.
- Net-log and diagnostics: net-log-window UI, showNetLogWindowEpic, net.log, netLogStatus, Copy installation ID menu item, Show tracing UI, Enable main process inspector - [SRC]. H.
- Speech: Speech submenu with Start speaking and Stop speaking (macOS speech roles) - [SRC] desktop_menu. M.
- Accessibility: desktop_client_accessibility_support trace, nativeTheme.shouldUseHighContrastColors (2), keyboard navigation submenu - [SRC]. M.
- Localisation: nine languages (de, en with GB base, es, fr, it, ja, ko, pt, zh); six namespaces (browser, external, general, menu, messagebox, renderer); hashed message ids; lint:locales script - [SRC]. H.
- Theme: systemTheme and userTheme preferences; nativeTheme use - [SRC]. M.
- Printing and PDF export: none found (printToPDF 0) - [SRC absence]. M.
- Command center shortcut (Ctrl or Cmd+Shift+K): registered global shortcut and renderer event DESKTOP_COMMAND_CENTER. UI and purpose not located. In-app Ctrl+Shift+K is documented as compose-new-message - [SRC] M for registration; [DOC] search summary M for the in-app meaning; purpose L.

### 3.12 UX patterns and layout (taxonomy 12)

- Left workspace rail with team icons: team-list-end-separator; keyboard next and previous team; tray team list - [SRC]. H.
- Settings editor window: table#settings with settings-row rows and computed-value labels; opened from the Open Settings Editor menu item - [SRC] settings-editor.bundle.js; desktop_menu. H.
- Loading and error states: desktop_loading_screen constant; "Slack is having trouble loading" dialog with wait, reload or restart choices; crash text asking to restart Slack; app://error page - [SRC]. H.
- What's New and release notes: showReleaseNotesEpic, What's New menu item, collect-release-notes script - [SRC]. H.
- About window: about-box.html and about-box.bundle.js - [SRC]. H.
- Child and in-app browser windows: child-preload, windowType childWindow (1) and browser-window (4) with back and forward (desktop-browser-forward and desktop-browser-backward) - [SRC]. H.
- Basic-auth and net-log windows: basic-auth-view and net-log-window UIs - [SRC]. H.
- Slackbot Mini tray-anchored window: see 3.7 - [SRC]. M.
- Command-center overlay: see 3.11; UI not located - [SRC]. L.
- Window chrome on macOS: trafficLightPosition x 14 with frameless or hidden title bars - [SRC]. H.
- Notification window styling: notifications.html with inline styles and the Lato and slack-icons fonts - [SRC]. M.

## 4. Extension points

- Desktop bridge for the web app: window.desktop exposed through contextBridge.exposeInMainWorld("desktop") in C (C@618065); window.slackPlatform exposed in P (P@762422, carries an isWayland flag) - [SRC]. H.
- Sentry bridge: window.__SENTRY_IPC__ exposed (2 hits) - [SRC]. H.
- IPC surface: channel "redux-action" (M@222740) plus enum-named channels such as CLOSE_CALL, NOTIFY_GLOBAL_SHORTCUTS_TRIGGER, FORWARD_DEEP_LINK, CUSTOM_MENU_ITEM_CLICKED, GET_NETWORK_INFO, POWER_SAVE_BLOCKER_START, WRITE_STRING_TO_CLIPBOARD, ZOOM_IN, SLEEP_AND_WAKE, SPEED_LIMIT_CHANGE, START_SEARCH - [SRC] M and C. H.
- Global-shortcut registration API: ACCELERATOR_REGISTER_DOUBLE_PRESS, GET_GLOBAL_SHORTCUTS_REGISTERED, TOGGLE_GLOBAL_SHORTCUTS_HANDLER, ACCELERATOR_CHECK_ACCESSIBILITY_PERMISSIONS, with input validation in M - [SRC]. H.
- Deep links: the slack:// scheme (see 3.6) - [SRC]. H.
- Custom app menu entries driven by the web app: CUSTOM_MENU_ITEM_CLICKED path (M) - [SRC]. M.
- Native addon surface: @tinyspeck/slack-desktop-utils (node-pre-gyp; Windows and macOS branches), native-keymap, file-handler-info, electron-native-auth, macos-notification-state, windows-focus-assist, registry-js, cf-prefs - [SRC NM]. H.
- Electron utility processes: slack-focus-assist-check, slack-notifier-state-check, slack-media-access-check (utilityProcess.fork with serviceName, M@972777) - [SRC]. H.
- Chromium command-line switches read at start: app.commandLine (24 uses) - [SRC]. H.
- Build-time hook: publish-desktop-types script in pkg (could publish types for the web client; purpose INF) - [SRC]. M.
- Dev and test harnesses: spec/e2e and spec/old-joe workspaces, test and test:rspack scripts, try-electron - [SRC pkg]. H.
- Toolbelt OAuth (developer integration): see 3.6 - [SRC]. M.
- Public Slack platform (DOC): Web API, Events API, Socket Mode, Block Kit, Bolt SDKs, Slack CLI, AI apps and agents, Slack MCP server, Slackbot MCP client (docs.slack.dev pages; see 3.6 and 3.7) - [DOC]. M.
- Public admin APIs (DOC): Discovery API on Enterprise Grid, SCIM, SAML SSO - [DOC] slack.com help articles (search summaries). M.
- Slack skills plugin for Claude Code and Cursor (DOC): mentioned in a docs.slack.dev AI summary - [DOC] L-M.
- Webhooks, slash commands and workflow steps: not verified in this session (no shell code and no doc fetch) - [INF] L.

## 5. Data and protocol hints

- Local storage (desktop): app.getPath is called 33 times (userData among them). It holds the persisted client store (slack-persistor name), a toolbelt-tokens file (secret-bearing, NOT read) and an auth-cookie-backup feature name (secret-bearing, NOT read). Service-worker and cache clearing actions exist. Electron session partitions are used via session.fromPartition (3) - [SRC] M.
- Secrets at rest: safeStorage encryptString and decryptString, with the OS backend chosen by getSelectedStorageBackend (2) - [SRC]. H.
- Logs: app.setAppLogsPath (4), net.log (2), Show Logs menu entries, log-export text - [SRC]. H. winston is a dev dependency only - [SRC pkg].
- Wire formats: Electron IPC with structured messages; Redux action objects on "redux-action"; Sentry envelope names in IPC (ENVELOPE, EVENT, ADD_METRIC, SCOPE, STATUS, RENDERER_START) - [SRC]. H.
- Network hosts (from strings): slack.com and app.slack.com; assets *.slack-edge.com (a., b.) and slack-imgs.com; files slack-files.com; messages slack-msgs.com; regional families slack.eu, slack-gov.com, slack.mil and their -dev variants; spellcheck.slack-edge.* - [SRC]. H.
- Auth flows: direct-SAML mode (PLATFORM_NATIVE_DIRECT_SAML at M@789602) with /ssb/signin and /sso/saml/start?redir=; magic login; toolbelt OAuth on toolbelt.tinyspeck.com; auth.tinyspeck.com host (not analysed) - [SRC]. H.
- Update feed: updater_variant query value msix or squirrel and an arch=arm64 flag; feed URL set at runtime (setFeedURL) - [SRC]. H.
- Packaging formats: Squirrel.Windows (update.exe), MSIX/AppX, MSI (WiX), DMG, ZIP, Linux packagecloud; Windows Start tile manifest in the install resources - [SRC pkg; install]. H.
- Sync model: Redux store and epics in the main process; chat data is owned by the renderer web app, so the shell does not hold message stores - [INF] M.
- Integrity: launch check with modifiedDateAtLaunch; Electron fuses applied at packaging - [SRC]. H.
- Telemetry: Sentry crash and error envelopes; desktop trace events; session metrics; feature-flag load event - [SRC]. H.
- Feature flags seen in the shell: focus_sessions, force_native_notifications, appActionsViaDeepLinks, switchClientFeatureViaSignOut, sign_in_by_default, feature_desktop_extend_app_menu, releaseChannelOverride, systemTheme, desktop_command_center. desktop_feature_flags_loaded logs a sorted list of the active flags - [SRC]. H.
- Managed preferences (macOS): cf-prefs - [SRC]. H.
- Runtime: Electron 44.4.5 (devDependency), Node >= 24 (engines) - [SRC pkg]. H.
- Not visible locally: WebSocket or other message transport, message and event schemas, E2EE, token exchange, offline database. These live in the remote web client - [INF]. H.

## 6. Blockers and limits

- The chat product is remote. Channels, huddles, canvases, lists, workflows, Slackbot and the admin console are not in the extract. Claims about them rest on DOC search summaries and carry M or L confidence.
- REA coverage is partial and budget-limited. Literal IPC channels recovered: 0. Trace seeds returned no-match. The modules page covered 30 of 648 items. Do not rely on REA for IPC or feature names in this build.
- Bundles are minified; names come from string constants, enum values and locale keys. Control flow was not reconstructed. No source maps were shipped (sourceMappingURL comments point at absent .map files).
- Presence is not enablement. Many features are feature-flagged and flag defaults cannot be read offline.
- DOC is from web-search summaries (secondary), not full page fetches. Some claims come from third-party pages; they are marked M or L.
- No dynamic run. The app was not launched and no UI was observed.
- Secret-bearing stores were NOT read: toolbelt-tokens, auth-cookie-backup and any userData content. Only the names are recorded.
- Windows MAX_PATH: native jq failed on the extract path (longer than 260 characters). Reading through cat piped into jq worked. Use cat or bash builtins for that path.
- Install folder: only resources/ was listed. app.asar in the install was not opened; the extract is assumed identical (same version, not hash-verified).
- The extract LICENSE is 0 bytes. The four native .node addons were not opened.
- Windows (MSIX, Squirrel) and macOS-only branches are present but untested.
- Unresolved conflict: Ctrl+Shift+K is registered as a global shortcut in the shell, while DOC (search summary) puts compose-new-message on it in-app. Purpose of desktop_command_center unverified.
- REA tool-result files and this session's outputs were the only non-extract files read (jq header probes); no other session or credential data was opened.

## 7. Notable for the later deep dive (short list)

- Highest-value pointers: M@995144 (global shortcut map), M@1210075 (Slackbot Mini tray handler), M@222740 (redux-action IPC), M@789602 (direct SAML), M@1175614 (integrity check), M@1388759 (combo/double-press registration), P@762422 and C@618065 (bridges), M@1441477 (updater variant).
- Desktop-visible surfaces that a Verevon Space could be compared against: tray-anchored Slackbot Mini window, global mute and command-center hotkeys, workspace rail with Ctrl+1..9 switching, per-workspace SAML, native notification and Focus Assist awareness, call sound pack and media-permission helpers.
- Chat-level features (threads, huddles, canvases, lists, workflows, clips, AI agents, admin) need the web client or DOC; do not treat their absence here as absence in Slack.
