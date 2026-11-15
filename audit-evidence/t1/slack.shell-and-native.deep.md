# Slack desktop 4.52.178: shell-and-native deep dive (Audit 1, Verevon Space)

Facet: shell-and-native. Role: deep-dive analyst. Date: 2026-10-13. Read-only audit.
Companion file: `slack.shell-and-native.features.json` (126 records, ids `slack-shell-and-native-001` to `-126`). Record ids are cited below as `#NNN`.

Provenance tags: **REA** = observed through an REA tool on the shipped artifact; **SRC** = read directly in the shipped files (minified bundles, preloads, locale files, package.json, AppxManifest, the installed Slack.exe bytes); **DOC** = public Slack documentation; **INF** = inference (basis stated). Most claims here are SRC: REA could confirm structure but could not see IPC names, feature names or deep links in this app (see section 1.3).

---

## 0. Summary in ten lines

1. Slack desktop is a thin, very hardened Electron 44.4.5 / Chromium 152 shell around the remote slack.com web client ("Sonic"). Chat features (threads, huddles UI, canvases, search, agents) are NOT in the shell (SRC absence, #116). What the shell owns is the OS boundary: windows, tray/badge, notifications, deep links, auth handoff, updates, policy, diagnostics, security.
2. The shell is a main-process Redux store with about 150 named redux-observable epics (#046, #047), a 136-channel IPC surface where every channel has an origin gate and a Joi argument schema (#089), and one `window.desktop` bridge (#041) exposed only to trusted Slack origins (#090).
3. Best-in-class pieces: the IPC gate (#089), the hardened window factory with runtime enforcement (#088), notifications (rich Windows toasts, inline reply, failover, health report, OS Focus Assist awareness; #064 to #070), four-layer managed configuration (#025), server-driven updates (#019), data-lifecycle rules across sign-out and runtime upgrades (#055), and the "renderer never sees file paths" recent-files shelf (#114).
4. Realtime/offline is mostly in the web client; the shell contributes signals: online / slackDown / offline (#050), an activity-driven keep-warm "tickle" every 10 s (#051), sleep/wake and thermal events (#052), blank-page reload with quadratic backoff (#053), and a service worker plus OPFS footprint (#056, INF).
5. Calls plumbing is native and specific: key-down/key-up combo hooks for push-to-talk (#078), a native screen-share border overlay (#079), media-permission probes in a utility process (#081), a power-save blocker bound to a MessagePort (#083), a "safe to restart?" veto for huddles and uploads (#085).
6. Corrections to the scout inventory (section 1.4): the tray "Slackbot Mini window" is an overlay inside the main window, not a separate window (#010); the "integrity check" detects a replaced binary, not tampering (#036); Electron runtime 44.4.5 is now verified from the installed exe (#097).
7. Verevon today has no native shell at all (web SPA): no OS notifications or badges, no deep-link consumption for threads, 6 s polling, no push, no per-org client policy contract, no support bundle (baseline UI report: notifications, realtime and client-shell sections).
8. Highest-value items for Verevon that do not need a native shell: a validated deep-link grammar, one normalised attention payload with an aggregate counter, resume/visibility/activity resync, a three-state connectivity model, "locked by policy" flags per setting, a redacted support bundle, and keyboard parity (sections 9.1 and 9.2).
9. If Verevon adds a native host later, copy the security architecture almost verbatim (#088, #089, #090, #091, #093, #094, #097) and the "named capability allowlist" idea (#017, #018); do not copy remote Chromium overrides without signing (#042) or the device-profiling breadth of the telemetry (#057).
10. Weaknesses: heavy flag-gating with unreadable defaults, several shipped stubs (macOS DND helper returns false, OS search indexing reports unavailable, stealth restart epic empty), shared browser session for all tenants (#049), Windows-specific workarounds (focus steal through rundll32, 8 ms drag polling), and no source maps.

---

## 1. Method, evidence and corrections

### 1.1 What was analysed

- Pre-extracted copy of `resources/app.asar` (version 4.52.178, package `slack-desktop`, commit `5f3348f`): `dist/boot.bundle.cjs` (61 KB), `dist/main.bundle.cjs` (1.7 MB, 84 lines), `dist/preload.bundle.js` (764 KB), `dist/component-preload-entry-point.bundle.js` (618 KB), `dist/child-preload-entry-point.bundle.js`, `dist/magic-login-preload-entry-point.bundle.js`, four tiny utility-process entry points, five component HTML windows with bundles, 9 languages of locale JSON, `node_modules` with 8 runtime native packages.
- The installed MSIX folder (read-only): `AppxManifest.xml` and `app/Slack.exe` (246 MB; I scanned bytes for the Electron fuse sentinel and the runtime version string only, nothing was executed or modified).
- Public Slack docs through WebFetch/WebSearch (DOC): keyboard shortcuts, notification and DND help, desktop configuration, MSI/MSIX deployment, connection requirements, huddles, Flannel engineering post, Socket Mode.
- Verevon context: `findings/t1/v-space-ui.baseline.md` and `v-space-backend.baseline.md` (sister audits of C:/dev/CoresSystem; I read their summaries and grep-selected sections, not the code).
- Not read (rule 3): anything in per-user data directories, cookies, token files (`toolbelt-tokens`, auth cookie backup, `dpop-keys` contents). Only their names appear in code and are recorded as names.

### 1.2 How to resolve evidence pointers

Evidence strings for minified bundles look like `slack/dist/main.bundle.cjs#c1209808 handleSlackbotMiniClick`. `c<N>` is a JavaScript string index (UTF-16 code units) into the file read as UTF-8; it is NOT the byte offset used by the scout (`@N` from grep -b), which drifts after the first multi-byte character. To resolve: `node -e "const s=require('fs').readFileSync('<extract>/slack/dist/main.bundle.cjs','utf8');console.log(s.slice(N-200,N+600))"`, or search the label text. For JSON, HTML and package files the pointer is `path:line`. Anchors were resolved by the generator at write time (it fails if a string is absent), so every pointer exists in the extracted copy. Extract root: `.../scratchpad/extract/slack/`.

### 1.3 REA usage log (honest)

| REA tool | Use | Result |
|---|---|---|
| `analyze_javascript_application` | 1 run, directory format on the extract, `detail=summary`, `integrity_policy=record-and-continue`, heap 2048 MB, timeout 600 s | Parent evidence `ev_203fd847...e153e7f`, view `ev_82c8f3e1...29eb37`. 662 relevant files, 454 parsed JS files, 1517 findings, 0 parse failures, `modules=0`. Electron block: browser_windows 0, explicit_web_preferences 0, preload_entrypoints 0, context_bridge_apis 1, ipc operations 3 (all dynamic channel expressions, literal_channels 0), renderer transmissions 2, listeners 1, main handlers 0, sender_validation_observations 4, utility_processes 0, native_addon_bindings 0. Coverage partial (semantic budget 0; 9828 semantic unknowns). |
| `inspect_analysis_view` | modules page 0-12 (result 836 KB, saved to file), then item view by path | Page lists node_modules assets only; item view by path refused because `dist/main.bundle.cjs` matches ~46 module nodes (must select by node_id). No further value. |
| `trace_application_feature` | seeds: api `exposeInMainWorld`, string `slack://` | Both `no-match` on a partial graph (evidence `ev_546b8e0f...`, `ev_f3986779...`). REA's graph did not capture the deep links, the contextBridge call or any literal IPC channel that direct reading found. |
| `inspect_pe_resources` | `slackdesktoputils.node` | PE32+ x64, one RT_MANIFEST (evidence `ev_2a90444f...`). Used as REA evidence for the native-addon record (#126, provenance REA, blended with SRC). |
| Not used | `open_binary`, `binary_overview`, `search_strings`, `list_strings`, `inspect_artifact`, ADB, web-capture tools | Shared-session risk and no need; the native module names were read from the .node strings with a script. |

Conclusion: for Electron apps with rspack-style minified bundles, REA here gave a coverage-limited structural summary (counts of bridges, ipc operations, sender validation) that is consistent with, but far thinner than, direct reading. All feature-level claims below come from SRC unless tagged otherwise.

### 1.5 What was not done

No dynamic run (rule 1), so nothing about actual flag defaults, server responses, UI rendering or runtime behaviour is verified. The web client (messages, threads, huddle UI, canvases, search, Slackbot) is remote and absent from the extract.

### 1.4 Corrections to the scout inventory (`slack.inventory.md`)

| Inventory said | Verified |
|---|---|
| "Tray click opens the Slackbot Mini window" | The tray click handler (`handleSlackbotMiniClick`) raises the MAIN window and sends `NOTIFY_GLOBAL_SHORTCUTS_TRIGGER(desktop_command_center)` to it. The log text still says "Slackbot Mini window" (naming legacy). The overlay is web-rendered. With the flag off, click only foregrounds the app. (#010) |
| "Launch integrity check / tamper check" | `INTEGRITY_CHECK` compares the executable's mtime at launch with its mtime after a child/renderer process crash (60 s throttle). It detects "binary replaced while running" and tags crash reports; it is not tamper detection. (#036) |
| "Electron runtime assumed 44.4.5 (devDependency)" | Verified from Slack.exe: `Chrome/152.0.7977.130 Electron/44.4.5`. `slack-desktop-utils` itself declares target Electron 42.2.0 headers (N-API v8, so ABI-stable). (#097, #126) |
| "REA IPC view is incomplete; enum-named channels such as ..." | Confirmed, and extended: the full channel enums, the 136-entry validation map and the bridge namespaces were extracted (sections 3 and 4). |
| "Single or multiple session partitions" | One shared default session for all workspaces; a separate `slack-component` partition for local HTML windows. (#049) |
| "macOS DND helper looks stubbed" | Confirmed: `call-get-macos-notification-state-entry-point` always posts false; the Windows Focus Assist path is real. (#072) |
| "Stealth restart" not in inventory | IPC contract exists (huddle/upload/guard veto) but the epic body is empty in this build. (#085) |
| "Local file search, Spotlight handoff unknown" | IPC `IS_SEARCH_INDEXING_AVAILABLE` is hard-coded false here; add/remove index items pass through to a native search module when loaded; `spotlightHandoff` and `canvasHandoff` epics are empty stubs. (#108) |
| Fuses "devDependency only" | Fuse state read from the exe: 9 fuses `010011001` (section 7). (#097) |

---

## 2. Architecture

### 2.1 Process and window model (SRC)

```
Slack.exe (Electron 44.4.5, Chromium 152, MSIX x64, fuses locked)
 |- boot.bundle.cjs ........ CLI flags, userData profile, requires main
 |- main.bundle.cjs ........ Redux store + ~150 epics, windows, tray, menus, IPC (136 channels),
 |                           updater, deep links, notifications, telemetry, policy readers
 |    |- utilityProcess: slack-focus-assist-check, slack-notifier-state-check, slack-media-access-check
 |    |- native addons (8 .node, N-API): slack-desktop-utils (in-house) + 7 small helpers
 |- Renderer: MAIN window  (session.defaultSession, preload.bundle.js, remote https://app.slack.com/client/...)
 |- Renderer: child windows (window.open from web app; child-preload: only slackPlatform.isWayland)
 |- Renderer: component windows (app://resources/*.html, partition "slack-component", component preload,
 |             strict CSP+SRI): notifications, about-box, settings-editor, basic-auth-view, net-log-window
 |- Hidden windows: magic-login window, download window, TSAuth window (dev), cloud-picker child windows
```

- One main web window hosts ALL workspaces. The main URL is the app host with path `client/<selectedWorkspaceId>` for org-aware accounts, or an `/ssb/...` boot path with `redir=/ssb/client` plus `lsp` and `aswebauth` query flags otherwise ("getSonicBaseURL/BootURL", #101). Workspace switching is done by the web app in response to shell commands (#040).
- Window state is a typed record per window (id, type, frame{fullscreen, maximized, bounds, title, audible, alwaysOnTop, contentProtected}) in redux `windows`, with window types main, webapp, notifications, about-box, settings-editor, basic-auth-view, net-log-window, chrome, other (SRC enum).
- Each web view has a lifecycle state (unloaded, loaded, webapp-loaded, empty-page, page-fail-load, page-closed, render-process-gone, webapp-session-expired, page-load-timed-out) and a content type (app, auth, posts, report_issue, box, dropbox, gdrive, onedrive) (#054, #118, #122).

### 2.2 Shell state and epics (SRC, #046 to #048)

- Redux store in main; slices seen in the preload replica: app, downloads, events, environment, menus, notifications, settings, teams, userWorkspaces, webapp, webContents, webContentsAssociations, windows, workspaces, workspacesMeta (+ performanceTiming, diagnostics).
- Preloads run a replica store: they boot from `ipcRenderer.sendSync(GET_REDUX_STATE)` and forward non-local actions on the single channel `"redux-action"`. Action creators carry `meta.logPayload` selectors so logs only include allowlisted fields.
- About 150 epics, named in an enum (MAGIC_LOGIN, SECOND_INSTANCE, INTEGRITY_CHECK, TRANSFORM_DEEP_LINKS, SYSTEM_IDLE_STATUS, TICKLE_MESSAGE_SERVER, HEAL_DISPLAY_SCALE, RELOAD_EMPTY_PAGES, SET_BADGE_EPIC, WORKSPACE_SELECTION, START_NATIVE_AUTH, CLEANUP_VERIFIED_DOWNLOADS, ...). Each is wrapped with error logging; an `epic-trace` debug feature emits rxjs_epic_trace telemetry.
- Persistence: `userData/storage/root-state.json` (redux-persist, throttle 8 s, version, per-slice field whitelists) and `local-settings.json` (sync KV: appCommandLineSwitches, electronFeatureOverrides, nextExpectedMinAppVersion, clientEnvironment, lastEffectiveClientEnvironment). Instance and tray UUID files (base64) identify the install in telemetry.

### 2.3 The web-to-native bridge (SRC, #041, #090)

`window.desktop` (alias `window.winssb`, "SSB" = site-specific browser) and a reduced `window.process {versions, mas, sandboxed}` are created with `contextBridge.exposeInMainWorld` in the preload, only when the page URL is an https Slack-domain origin on port 443 (or the app:// pages, or an opener-trusted pop-up). Namespaces: window, accelerator, accessibility, app, calls, callsScreenshare, clipboard, contextMenu, contextMenus, diagnostics, dock, downloads, localFileSearch, menus, notice, recentFiles, search, screen, spellCheckerV3, stats, teams, workspaces, redux, windowChrome, system. The reverse direction is delegates the web client registers: `setGlobalDelegate(obj)` and `exposeWorkspaceDelegate(workspaceId, obj)`; the shell calls handleDeepLinkWithArgs, openDialog, startSearch, closeCall, customMenuItemClicked, maybeTickleMS, focusNextWorkspace/PreviousWorkspace/setFocusedWorkspace, speedLimitChanged, queryStealthRestartSafe, systemTextSettingsChanged, accessibilitySupportChanged, onClosingChildWindows.

IPC inventory (names, SRC): the web preload uses 107 distinct renderer-to-main names: 82 invoke-style channels (ACCELERATOR_*, ADD_TO_SEARCH_INDEX, ASK_FOR_MEDIA_ACCESS, CAN_INIT_AUDIO_DEVICE, CLEAR_SESSION_CACHE, CREATE_CONTEXT_MENU, ENUMERATE_AUDIO_DEVICES, EXEC_BROWSERWINDOW_METHOD, GET_* diagnostics and state, NOTIFY, POWER_SAVE_BLOCKER_START, SEARCH_LOCAL_FILES, SET_AND_PASTE_CLIPBOARD, START_DIAGNOSTICS_TRACE, UPLOAD_APP_LOGS_ARCHIVE, UPLOAD_RECENT_FILE, WAIT_AUTH_COOKIE_CHANGE, ...), 26 fire-and-forget channels (ADD_NEW_TEAMS, REMOVE_TEAMS, SYNC_WORKSPACES, SELECTED_WORKSPACE_CHANGED, START_DOWNLOAD, UPDATE_SETTINGS, ZOOM_IN, TOGGLE_DEV_TOOLS, ...), and 13 main-to-renderer events (FORWARD_DEEP_LINK, CLOSE_CALL, SLEEP_AND_WAKE, SPEED_LIMIT_CHANGE, START_SEARCH, TICKLE_MESSAGE_SERVER, WORKSPACE_SELECTION, QUERY_STEALTH_RESTART_SAFE, ...). The main-process validation map has 136 Joi-schema entries because it also covers component-window, window-RPC, screen-share and telemetry channels.

### 2.4 Sessions and storage (SRC, #049, #055)

- Web app: `session.defaultSession` (id `PROD_DEFAULT_SESSION`); dev builds use `persist:NORMAL|ALTERNATE(+Dev|DevEnv)` partitions. Component windows: `slack-component` partition. All workspaces share cookies/storage (cookies `d` and `lc` are monitored; values never read).
- Sign-out clears caches, service workers, filesystems, webSQL (+ IndexedDB only when no workspace remains) and deletes per-domain `dpop-keys` files that are no longer needed (#055, #096).
- Runtime upgrade/downgrade handling: copies Cookies to `StaleCookies-<prevMajor>` on Electron major upgrade, restores it and deletes `Service Worker` on downgrade, and clears `Code Cache` on any version change (#055).
- TEMP fallback: if `app.getPath("temp")` is unusable the app creates `userData/temp` (SRC).

---

## 3. Protocols and flows

### 3.1 Deep links (SRC, #003 to #007, #102)

`slack://<cmd>?args`. Recognised: `open` and `channel` (team; the id parameter was not verified), `app` (action=reset-window|quit|reveal-logs|clear-cache-and-restart), `setting` (JSON, allowlisted 31 keys, Joi-validated; cannot set clientEnvironment), `login-v2` (indexed token sets `0.host`, `0.tokens`, `0.dpop`, `1.host`, ...), `/magic-login/<TEAMID>-<token>` paths, `toolbelt-callback` (dev OAuth code/state), `noop`, `devEnv=`, `openDevToolsOnStart`, `feature_*=1` and `pri=N` web params. `host` must pass a Slack-domain test or is dropped. Flow: macOS `open-url` or Windows `/p "<url>"` argument -> HANDLE_DEEP_LINK action -> TRANSFORM_DEEP_LINKS (map `team` through userWorkspaces) -> FORWARD_DEEP_LINK_TO_WEBAPP (wait for WEBAPP_LOADED, 10 s timeout) -> preload -> `desktopDelegate(s)[teamId].handleDeepLinkWithArgs`.

### 3.2 Sign-in and token handoff (SRC, #101, #102, #096)

Three methods (browser, platform-native, platform-native-direct-saml), `aswebauth=1` on the boot URL for native methods, interception of `/ssb/signin`, `/ssb/signin_redirect`, `/ssb/add` navigations, magic-login in a hidden window that shares the main session (preload returns `document.body.innerText` as `{ok|error}` JSON), parallel multi-workspace token processing from `login-v2`, optional DPoP flag, policy `DefaultSignInTeam`. Internal staff path: toolbelt/TSAuth OAuth with an OS-encrypted token file (#123).

### 3.3 Update (SRC, #019 to #023)

`GET {base}/desktop/update/v2/should-i-update?platform&version&os_version&channel[&os_product&servicing_channel][&arch][&updater_variant=msix|squirrel][&is_user_initiated]` -> `{update_available, version, url, why[]}`. Timer: every hour when update is supported; manual check from the menu; menu label state machine. Engines: Squirrel (RELEASES pre-check, then Electron autoUpdater) and MSIX (native `registerRestart` + `registerPackage(forceShutdown)`). A signed `SLACK_UPDATE_URL` override exists for testing (RSA-SHA256 over URL + timestamp, must be within 5 h of process start). After an update, `nextExpectedMinAppVersion` attributes a launch crash to the update.

### 3.4 Notification delivery and activation (SRC, #064 to #070)

NOTIFY (Joi schema: id, groupId, groupTitle, title, subtitle, body, avatarImage, sound enum, silent, urgency, hasReply, imageUri, launchUri, trace_id, actionButtons[], teamId, userId, msg, channel, channelName, thread_ts, authorName, content, groupWindowsNotifications) -> OS state gate -> NEW_NOTIFICATION -> native Windows toast built from `ToastGeneric` XML or the HTML window. Activation: click -> CLICK_NOTIFICATION with {channel, messageId, notificationId, teamId, traceId, threadTimestamp}; reply -> REPLY_TO_NOTIFICATION {.., response}; action button -> `slack://` launch URI or NOTIFICATION_ACTION index; header click -> team URL. Activation payloads are Joi-validated alternatives (notification | action | header). Failure ladder: 5 consecutive failures -> telemetry, `notificationFailover` timestamp, HTML fallback for a week.

### 3.5 Telemetry (SRC, #057, #058)

`track()` -> ReplaySubject -> bufferTime(70 s) -> URL-encoded JSON chunks < 1500 chars -> `{base}/clog/track/` (cap 50 chunks per window). `trace()` -> span-like payload (id, trace_id, start/duration micros, tags) with ssb_instance_id, platform, arch, distribution, app/electron/OS versions, virtualization, network type, memory, CPU model/cores, machine model, uptime, JS heap, process counts, user and team ids. IPC schema failures -> `{base}/beacon/ssbipc` (3 s timeout). Sentry (events + minidumps via Electron crashReporter) with a renderer IPC bridge. Desktop feature flags are polled hourly and also pushed from the web app (#043).

### 3.6 Managed configuration (SRC + DOC, #025 to #027)

Windows registry `HKLM then HKCU` under `SOFTWARE\Policies\Slack` (enforced) and `...\Slack\Defaults` (defaults); macOS managed preferences (domain `com.tinyspeck.slackmacgap`, `Defaults` dictionary); Linux `/etc/slack-desktop.conf` (JSON, root keys enforced, `defaults` object). Keys: AutoUpdate, ClientEnvironment (1000/1001; code also knows 1002), DefaultSignInTeam, DownloadPath, HardwareAcceleration, HideOnStartup, ReleaseChannel, SignInMethod (mac), WebSwitches. State layering: slackDefaults < itDefaults < userChoices < itPolicy; the web app is told which keys are enforced. Public docs describe the same keys, paths and HKLM-over-HKCU precedence (DOC).

---

## 4. Key data models (SRC, from Joi schemas in main module 75307)

- **Team state pushed by the web app** (`REDUX_UPDATE_FROM_WEBAPP`): `teams[teamId] {theme{titlebarBackground,titlebarTextColor}, notificationPrefs{notificationSound, noTextInNotifications, muteSounds}, connectivityPrefs{ntlmCredentialDomains}, featureFlags{string:boolean}, unreads{unreads, unreadHighlights, showBullet, teamId}, locale, isRecording, version, frontendBuildType, userId}`.
- **Workspace** (`SYNC_WORKSPACES`): `{id, name, domain, url, icon{image_34..image_original}, order, variant: commercial|gov}`; **user workspaces** (`STORE_USER_WORKSPACES`): `{userTeamId, workspaceIds[]}`; ids match `^(E|T)[A-Z0-9]{8,50}$`.
- **Sign-in team** (`ADD_NEW_TEAMS`): `{id, token, domain, name, team_id, team_name, team_url, user_id, usage, idle_timeout, showBullet, unreadHighlights, unreads, hasValidSession, theme{8 colours}, icons{...}}`.
- **Download**: `{id, teamId, userId, url, startTime, endTime, downloadState, downloadPath, progress, requestState, appVersion}` keyed teamId -> id; verified path registry rebuilt at start.
- **Menu item**: Electron fields + `order`, `before|after|beforeGroupContaining|afterGroupContaining`, `accessibilityLabel`, `badge{content,count,type alerts|updates|new-items|none}`; workspace-scoped menus keyed by workspace id (#106).
- **Window frame** state and **notification** payload as above; **diagnostics trace** request `{identifier, endpoint, token, mode, categoryFilter, captureAllProcess, metadata}`; **OS search index item** `{identifier, contentDescription, domainIdentifier, title, expirationDate, iconURL}` (Spotlight-shaped, #108); **clipboard formats** map (1 to 16 formats, each at most 10 MB) (#111); **network emulation** `{offline, latency, downloadThroughput, uploadThroughput, connectionType}` (#050).
- **Environment**: appVersion, arch, platform, platformVersion{major,build,revision}, isGpuCompositionAvailable, uuid, applicationProcessSessionId, distribution, bootFragments, clientEnvironment, invokedOnStartup, win32{osProduct, servicingChannel, isWin10, isPackagedApp}.

---

## 5. Notifications and attention (categories 5, parts of 11)

What Slack's shell does that most chat shells do not (all SRC unless noted):

1. A single normalised payload for all notifications, with `trace_id` carried from server to click (#064).
2. Rich Windows toasts: per-workspace grouping header, circular avatar, hero image, inline reply box, action buttons, packaged sounds on MSIX, COM activation (#065, #066).
3. Defence against malicious payloads: XML escaping, `slack://`-only launch URIs, image-host allowlist, validated activation (#098).
4. Respect for OS attention state: Windows notifier state (app/user/group-policy/manifest), Focus Assist with per-app priority, GPU composition; probes run in utility processes so a hung COM call cannot freeze the app (#069).
5. A "why am I not getting notifications" report with cache TTLs (#070) and a self-healing failover ladder (#067).
6. Aggregation: tray icon states and tooltip, taskbar overlay badge (101 pre-rendered PNGs) over highlights across all workspaces, one flash after 10 idle seconds (#009, #011, #012). DOC adds per-workspace badge selection, keywords, VIP, DND schedules and mobile push timing (#075, #076).
7. Limits: no digest or inbox in the shell (Activity is in the web client; DOC), macOS DND helper is a stub, notification preferences UI is remote.

---

## 6. Realtime, sync and offline (category 9)

- Transport is not in the shell. DOC: WebSockets over port 443 to wss-primary/backup/mobile.slack.com with Flannel edge caches for lazy loading (#062); apps use Socket Mode (#063). The shell mentions a "message server" only for the tickle (#051).
- Shell responsibilities: connectivity state (#050), activity/idle polling and keep-warm (#051), sleep/wake and thermal events (#052), reload policy (#053), per-webContents state machine (#054), local data lifecycle (#055), service-worker and OPFS footprint (#056, INF), telemetry batching (#057), startup phases (#058).
- Multi-device: nothing in the shell; read state and presence are server-side (DOC and INF).
- E2EE: none in the shell; chat data never enters the shell store (INF, #061).
- Scalability signals (DOC): ~4M simultaneous connections and 600K Flannel queries/s at peak, 44x smaller boot payload at 32K users, pub/sub presence cut presence events 5x.

---

## 7. Security posture (category 10, SRC)

| Control | Evidence | Verdict |
|---|---|---|
| Sandbox, contextIsolation, no nodeIntegration, no webview; non-overridable factory | #088 | Excellent; runtime guard destroys non-conforming windows |
| IPC gate: sender frame/process transition checks, origin allowlist by session, Joi schema per channel (136), failure beacon | #089 | Best practice |
| Bridge only for trusted origins; child windows get one flag | #090 | Excellent |
| Navigation/redirect allowlists, scheme allowlist, consent page | #091, #119 | Strong |
| Permission handlers default-deny with narrow exceptions | #092 | Strong |
| app:// path guard; CSP + SRI on local HTML; DOMPurify/Trusted Types in notification UI | #093 | Strong |
| Download trust: host allowlist, verified registry, dangerous-extension warnings | #094 | Strong |
| Certificates never bypassed | #095 | Good |
| Token binding (DPoP) hints, per-domain key lifecycle | #096 | Partial visibility |
| Fuses (installed Slack.exe, wire v1, "010011001") | #097 | RunAsNode off, cookie encryption on, NODE_OPTIONS off, inspect args off, embedded ASAR integrity on, only-load-from-asar on, V8 browser snapshot off, file-protocol extra privileges off, WASM trap handlers on (order per @electron/fuses v2: INF; bits certain) |
| MSIX capabilities | AppxManifest | runFullTrust + internetClient (full-trust app, not an AppContainer) |
| Log/state redaction, payload allowlists | #099 | Strong; avoid raw payload logging |
| Signed update override | #020 | Good test-path hygiene |
| Remote Chromium flag overrides | #042 | Powerful and risky; trust rests on the server channel |
| Shared session across tenants | #049 | Isolation is server-side only |

---

## 8. Client shell capabilities (category 11 and UX 12)

- Windows: tray with three states (#009), overlay badge (#011), taskbar flash policy (#012), jump list with workspaces (#008), custom web-rendered chrome themed by workspace (#014), launch at login honouring machine policy and MSIX StartupTask (#024), bounds and DPI healing (#015), drag loop (#016), Alt-mnemonic menus (#013).
- macOS-only branches are present (dock menu, traffic-light position, vibrancy, security-scoped bookmarks, AuthRequest, Spotlight handoff stubs) but unexercised here.
- Keyboard: Ctrl+1..9 workspaces, F6 region cycling, Alt+letter menus, global Ctrl+Shift+Space (mute) and Ctrl+Shift+K (command center) registered on demand by the web client (#028, #077); DOC lists the in-app model (#110).
- i18n: 9 shell languages x 6 namespaces (ICU plurals), 12 web locales, 10 MSIX resource languages (#031). a11y: screen-reader detection, high contrast, keyboard menus (#032). Spell-check: 45+ downloadable dictionaries (#113).
- Support: net log, verbose logs, tracing, GPU page, reset, log upload (#033, #034), crash/hang attribution (#035), self-diagnosing notifications (#070), failure pages (#120).
- Distribution: Store, MSIX, per-user Squirrel, machine-wide MSI (LMSI), DDL, MAS; UA carries the facts (#037, DOC deploy guide). Release channels prod/beta/dogfood (#023).

---

## 9. What this means for Verevon Space

Baseline (from the sister audits, tagged baseline): Space is a web SPA with a six-tab cockpit, 6 s polling while visible, SSE only for the sender's own turn, no notifications of any kind from Space events, no cross-room unread, no push, no native shell, thread deep links generated but not consumed, i18n partially hard-coded Norwegian, Cmd/Ctrl+K opens web search only. Strengths: authority honesty, fail-closed membership rechecks, governed agents with approvals, receipts. Product stance in docs rejects chat-feature parity races, which conflicts with the stated goal of a Teams + Slack + Grok hybrid (baseline UI report).

### 9.1 Copy now (web-first, no shell needed)

| # | Slack mechanism | Verevon action | Evidence |
|---|---|---|---|
| 1 | Validated deep-link grammar; resolve team through membership; queue until app ready | Define `verevon://` and https deep links for space, thread, run, approval; make SpacePage read `thread_id`; authorise via Control membership, never widen ACL | #004, #005 |
| 2 | One normalised attention payload with trace id and action buttons | Define a Space attention event (mention, reply, approval needed, run finished, agent question) shared by SPA, push and email | #064, #074 |
| 3 | Aggregate counter over all workspaces (highlights only) | One sidebar/tab/OS badge counting items that need the human (approvals, mentions), not raw unread | #011, #009 |
| 4 | Activity/visibility/resume-driven resync; sleep/wake events | Refetch on focus, resume, reconnect and on activity; show "resynced at"; stop relying on a bare 6 s timer | #051, #052 |
| 5 | Three-state connectivity (online / service down / offline) and bounded reload with backoff | Add explicit states and a retry ladder to the Space shell; distinguish gateway down from offline | #050, #053 |
| 6 | Per-setting "enforced by policy" flag; four-layer settings precedence | Control org policy > org defaults > user > product; ship `locked_by_policy` to the SPA | #025, #027 |
| 7 | Redacted support bundle (logs, settings layers, feature flags, notification health) | Add "Download diagnostics" with schema-path redaction (ZDR-safe) | #033, #034, #099 |
| 8 | Self-diagnosing notifications | "Alerts blocked by Focus mode / permissions" banner | #070 |
| 9 | Keyboard parity (workspace/space switching by number, F6 region cycling, quick switcher, jump to unread) | Specify and test in the cockpit tablist and rail | #013, #028, #110 |
| 10 | Message-id based ICU i18n across surfaces | Fix hard-coded Norwegian activity labels; shell-ready message ids | #031 |
| 11 | Locked-down local surfaces (CSP + SRI, path-safe custom protocol) | Apply to any embedded agent-browser panel or local viewer | #093 |
| 12 | Never trust notification or link payloads from members/agents | Escape, allowlist hosts and schemes, validate activation args | #098, #091 |

### 9.2 Adapt (PWA or thin host)

| Slack mechanism | Verevon adaptation | Note |
|---|---|---|
| Native toasts with inline reply and action buttons (#065, #066) | Web Push + service worker with actions for Approve / Deny / Open; evaluate whether inline text reply is feasible on target platforms | Feasibility of inline reply on desktop web push needs checking; mark as open question |
| Tray/global hotkey command center (#010, #077) | Installed PWA or thin shell with one global hotkey opening an "ask my agent" overlay | Fits the Grok-Bot-style personal agent goal |
| Protocol handler and single instance (#002, #003) | `web+verevon` protocol handler in the PWA manifest or host registration | |
| Aggregate badge (#011) | App Badging API for the installed PWA | |
| Server-driven update decision (#019) | If a host ships, ask Control/Model plane "should I update" with channel and cohort | |
| Stealth restart veto (#085) | Any host/agent update waits while a run, approval or upload is in flight; expose a "safe to restart" contract | Directly maps to agent runs |
| Screen-share border (#079) | "Agent is acting in this window" persistent border for agent browser/computer views | Strong safety cue |
| Recent-files shelf (#114) | Capability-by-id for agent file access; renderer never sees paths; confirmation before sharing | Matches "Space access never widens an owner-plane ACL" |

Browser/PWA capabilities named in this table (Web Push actions, App Badging, protocol handlers) are standard web-platform features that I did not verify against current support on Verevon's target platforms (INF).

### 9.3 If Verevon builds a native shell (Electron/Tauri)

Copy: hardened window factory plus runtime guard (#088), IPC gate with per-channel schema and failure telemetry (#089), bridge only for the product origin (#090), navigation and permission policy (#091, #092), fuses and signing in CI (#097), utility-process isolation for flaky OS probes (#069, #081), MessagePort-lifetime handles (#083), policy readers for registry/macOS/Linux (#025), notification failover ladder (#067), data-lifecycle rules per sign-out and runtime upgrade (#055), crash attribution to releases (#022, #036), signed override for dev feeds (#020), named capability allowlist for web-initiated windows and methods (#017, #018).

### 9.4 Avoid or treat with care

- Remote engine-level overrides (#042): require signing, audit and an org kill switch.
- Telemetry that profiles devices (CPU model, machine model, virtualization; #057): conflicts with ZDR minimalism; send counts and versions only.
- Shared browser session for all tenants (#049): acceptable only because every access is server-authorised; Verevon already issues per-effect Control decisions, so keep it that way.
- Opaque flag-gated behaviour (#043): log the effective flag set (as Slack does) and make defaults inspectable.
- Shipping dev affordances in production (#038), Windows-only hacks (focus steal via rundll32, 8 ms drag polling), and stubs presented as features (#072, #085, #108).
- Naming drift ("Slackbot Mini" vs command center, #010): keep one name per surface.

### 9.5 Gaps in Verevon Space that this facet exposes (verified against baselines)

No native notifications or badges (baseline §5); no cross-room unread; thread deep links ignored; no connectivity/service-down distinction; no resume/activity resync; no support bundle; no policy-lock contract to the UI; partial i18n; no global shortcut/command surface; no OS-level presence of agents in progress; no update/channel story (web SPA, so deploy-time only).

---

## 10. Best-in-class, weaknesses, table stakes

**Best-in-class (this facet):** IPC and bridge security (#088 to #090, #093, #094), notification stack (#064 to #070), enterprise manageability (#025, #027, #037, #104), update/crash attribution (#019, #022, #035, #036), data lifecycle (#055), supportability (#033, #034, #099), privacy-preserving recent files (#114).

**Differentiators vs a typical Electron chat shell:** native key-combo hooks (#078), screen-share border panel (#079), stealth-restart veto contract (#085), notification health report (#070), remote Chromium overrides (#042), signed update override (#020), server-driven update decision (#019), command-center tray/hotkey (#010, #077).

**Table stakes confirmed in the shell:** single instance, protocol handler, tray + badge, native notifications with click-through, auto-update, launch at login, multi-workspace switching, spell-check, i18n, crash/diagnostics, proxy auth, download manager (#002, #003, #009, #011, #064, #019, #024, #040, #113, #031, #035, #100, #117).

**Nice-to-haves:** jump list (#008), custom chrome theming (#014), About/What's New windows (#125), OS search-index plumbing (#108), numpad zoom and pinch lock (#028, #029), keyboard-layout tracking (#045).

**Weaknesses and limits:** the entire product value is in a remote client the shell cannot work offline without; wide flag matrix with unreadable defaults; stubs in this build (macOS DND helper, OS search availability, stealth restart epic, spotlight/canvas handoff epics); shared session; native addon set built for Windows even for macOS-named packages (INF); 1.7 MB single main bundle with no source maps; heavy device telemetry; internal dev auth code shipped (#123).

---

## 11. Open questions

1. What do `desktop_command_center`, `focus_sessions`, `async_dnd_check`, `windows_com_activation` default to in production, and what is the rollout percentage? (flag values are remote)
2. Does the web client store the message cache in IndexedDB/OPFS, and how does the Service Worker participate in offline? (INF only from shell plumbing, #056)
3. Where are DPoP keys generated and proofs signed (web, native, or electron-native-auth)? (#096)
4. What exactly does `slack-desktop-utils` `setAndPaste` paste into (composer vs another app), and which product feature uses it? (#111)
5. Which feature sets `isRecording` (#086) and what is the "Slackbot Mini" overlay's content (#010, #115)?
6. What is the scanned directory for local file search (`yT`) and who enables it? (#109)
7. Is the fuse ordering used for the 9-bit string exactly @electron/fuses v2's? (#097)
8. Which Windows-only behaviours differ on macOS builds (dock menu, `AuthRequest`, Spotlight)? Not exercised here.

---

## 12. Contribution to the cross-app comparison

For the Teams/Slack/Grok/buzz/qm synthesis, this facet supplies: (a) the desktop-shell table stakes list in section 10; (b) a reference implementation of the notification ladder and OS-attention awareness; (c) a reference security architecture for web-to-native bridges that any agent-hosting desktop (Grok Bot-style) should meet; (d) evidence that Slack keeps agents and chat features out of the shell, so Verevon's governed-agents-in-room model has no counterpart in this facet (#116); and (e) a list of Verevon web-first actions (section 9.1) that close real gaps without a shell. Quality scores in the JSON (1 to 5) are relative to a typical Electron chat client and to what was seen in this audit; no peer shell was re-inspected in this file, so cross-app calibration should be done in the synthesis step.
