# Manus Studio 2.0.6 (Windows) - inventory, AUDIT 3 scout

Scope: inventory of the Manus desktop app as a reference for Verevon's chat and agent pages and the Model Plane (chat, multimodal, research, agent runtime, tools, computer control, coding, memory, models, transparency, safety, UI, platform). Read-only audit. No app was launched, no install file was modified, and nothing was extracted into the install directory.

Locations:
- Extracted copy (read-only for this audit): scratchpad/extract/manus/ (package.json, dist/ with 231 files, frontend/out/ renderer static export of 124 MB, node_modules/ with 260 top-level packages, assets/).
- Install (read only, program files): C:/Program Files/WindowsApps/ManusAI.Manus_2.0.6.0_x64__vajzd2mq3s8wj (resources/app.asar 262 MB, resources/app.asar.unpacked/, resources/bin/, resources/addons/, resources/managed-addons/, resources/locales/ with 55 .pak files, Manus.exe 239 MB Electron binary, not executed and not analyzed natively).
- Verevon orientation read: C:/dev/CoresSystem/CLAUDE.md (full) and apps/CODEBASE_INFORMATION_SYSTEM.md (first 90 lines). No claim below depends on them.

Provenance tags used throughout:
- [REA] = output of an REA tool run on the shipped artifact (see section 0).
- [SRC] = read directly in shipped source or files: dist JS, @next-agent TypeScript src, package.json, manifests, tar listings, or grep over shipped files. Binary grep hits are tagged SRC with "binary strings".
- [DOC] = documentation text shipped with the app (SKILL.md, guide.txt, addon description).
- [INF] = inference; the basis is stated. Low-confidence items say so.

Renderer keyword counts (section 3) are substring hits over minified chunks. They show that a term exists in the bundle; they do not prove that a feature works or is reachable.

## 0. Provenance and REA usage log

- [REA] analyze_javascript_application: format=directory, input = extracted dist/ (main process, not the 262 MB asar), detail=summary, integrity_policy=record-and-continue, max_heap_mb=2048, analysis_timeout_ms=900000. Result: 231 files parsed, 2.18 MB of text, 239,261 AST nodes, 1,354 findings, 436 unknowns, 432 limitations (201 of them are "resource constraint: expanded application relationship" limits, so the graph is partial). Electron summary: 15 BrowserWindow sites, 59 explicit webPreferences, 2 preload entrypoints, 3 contextBridge APIs, 46 IPC operations (12 literal channels, 33 dynamic channel operations, 29 renderer transmissions, 11 unpaired literal renderer transmissions, 11 main handlers), 7 sender-validation observations, 1 utility process with a resolved entrypoint, 0 native addon bindings. The summary field "modules": 0 is not reliable.
- [REA] inspect_analysis_view on the retained analysis evidence: modules page, offset 0 to 60 of 394 items. Items are javascript-asset and javascript-module nodes (for example browser/browserPdfPageState.js, addonLocal/localAddonActivationOperation.js, computerUse/skillRecordingRpc.cjs).
- [REA] trace_application_feature, five runs:
  - channel "desktop-ipc-api" (exact): no match on the partial graph.
  - string "manus-computer-operator" (contains): no match.
  - route "presenter" (contains): no match.
  - api "api.manus.im" (contains): no match.
  - module "agentManager/operatorManager.js" (contains): 3 seed matches, 836 traced nodes, 992 edges, 439 terminal paths, coverage partial. Node kinds: unknown 411, javascript-asset 222, javascript-module 172, endpoint 28, browser-window 2, artifact 1. Edge relations: contains 672, imports 209, exposes 82, calls 29. The endpoint node keys recovered from this trace are listed in 3.4 and 3.5.
  - REA states that a no-match is not evidence of absence. The IPC, route and API claims below therefore rest on SRC reads, not on REA traces.
- Not used, with reasons: inspect_artifact (would inventory the whole 262 MB asar; the extracted copy already covers it); inspect_pe_resources, binary_overview, search_strings, list_strings (no native document was opened, to avoid the shared REA active-target session and a 239 MB binary analysis); analyze_javascript_application on frontend/out (124 MB minified renderer; one heavy analysis per app).
- Tool notes for the next agent: jq must read from stdin, because the native jq could not open the tool-results paths. Node cannot start when the working directory is the deep extract path (Win32 cwd length limit), so node was not used.

## 1. Identity, build, stack, dependencies

Identity
- [SRC extract/manus/package.json] name "Manus", productName and displayName "Manus Studio", version 2.0.6, license MIT (declared), desktopName im.manus.ManusStudio, main dist/index.js, exports ./ipc pointing to backend/ipc/registry.ts and ./preload pointing to backend/preload.ts. Those backend .ts sources are not in the copy (see section 6).
- [SRC install AppxManifest.xml] Identity ManusAI.Manus 2.0.6.0 x64, publisher "Manus AI", packagedClassicApp, runFullTrust, mediumIL, protocol handler "manus", Windows.Desktop minimum 10.0.19041.0.
- [SRC dist/utils/envHelper.js] Prod: appName "Manus Studio", appUserModelId im.manus.desktop, deep-link protocol "manus", single-instance key manus-desktop, update feed https://download.manus.im/. Dev build: "Manus Studio Preview", im.manus.desktop.preview, protocol "manusdev", feed https://d3n5o1ob208hj3.cloudfront.net/.

Build metadata
- [SRC install resources/bin/build.json] builtAt 2026-10-08T15:23:23Z, variant release, Go go1.26.4 windows/amd64, cache key recorded. Repositories: manusCli (ref main, commit 91d3f0e8...), manusSandbox (sbx-go-svc mcphost v0.0.0-20261001070504-601f84904f36 and executor-protocol v0.0.0-20261005082100-ab7ba5cd7a27), manusUtilities (ref main, commit 0eec85b1...). Built Go binaries with sha256 checksums: manus-computer-operator.exe 28,269,568 bytes; manus-utilities.exe 42,701,312 bytes; sidecar.exe 11,678,208 bytes.
- [SRC binary strings, Manus.exe] Electron/43.6.0 and Chrome/150.0.7871.250.

Stack
- Main process: JavaScript (CommonJS compiled from TypeScript), dist/ with 231 files and 2.7 MB. TypeScript source is shipped only for two workspace packages (below).
- Renderer: Next.js static export (basePath /desktop_frontend, buildId pqAsTRVPik4syjXDEqeMO, 1,414 files under _next/static, React-based). Routes present: app, login, presenter, screen-live-viewer, desktop-activity-indicator, video-entry, terminal-font, sounds, plus 404 and _not-found. [SRC frontend/out, __next._tree.txt and RSC text files]
- Workspace packages with TypeScript src/ shipped under node_modules/@next-agent: browser-runtime (50 src files) and screen-live-service (57 src files, 26 of them tests). [SRC]
- Go binaries (manusCli, manusUtilities and sandbox families; symbol-level only): manus-computer-operator.exe, manus-utilities.exe, sidecar.exe. [SRC binary strings]
- Rust: the screen-live publisher (manus-screen-live-publisher.exe, spawned by ScreenLiveRustProcess) and manus-remote-input.exe. Both sit in app.asar.unpacked/node_modules/@next-agent/screen-live-service/dist/native. [SRC]
- Native module: node-pty (app.asar.unpacked/node_modules/node-pty). [SRC]
- Bundled native tools: ffmpeg.exe inside the manus-video-editor addon; QuickJS (quickjs-emscripten) inside the computer-use addon runtime; ManusComputerUseHelper.exe (win32-x64 and win32-arm64). [SRC]
- PowerShell helpers: dist/utils/windowsPowerShell.js. The operator's orphan cleanup runs powershell.exe with -ExecutionPolicy Bypass. [SRC]

Declared dependencies (extract/manus/package.json), grouped by capability:
- Agent and tool protocols: @modelcontextprotocol/sdk 1.29.0; zod 3.25.76; workspace @next-agent/browser-runtime (which carries its own MCP SDK 1.29.0); workspace @next-agent/screen-live-service.
- Computer control and media: @next-agent/screen-live-service (declares livekit-client ^2.20.2 and zod 3.25.76); node-pty 1.2.0-beta.15 (terminal); qrcode-generator 1.5.2 (QR login or pairing; INF from the name and dist/desktopLoginToken.js).
- Browser and identity: @steipete/sweet-cookie 0.4.0 (browser cookie reader, used by dist/browserDataImport/SweetCookieReader.js); @beeper/webauthn-authenticator 0.2.1 (passkeys, used by dist/browser/passkey/browserPasskeyUtility.js); tldts 7.4.10 (domain parsing); @stripe/link-cli 0.23.1 (operator payment CLI, see 3.6).
- Desktop shell and updates: electron-log ^5.4.0; electron-updater ^6.3.9; electron-window-state ^5.0.3; app-root-path ^3.1.0; sc-prepare-next ^1.0.3 (isDev and environment detection in windowUrlHelper.js); compare-versions 6.1.1; semver 7.7.3.
- Networking and realtime: socket.io ^4.8.1 and socket.io-client ^4.8.1 (used by myComputerService/socketClient.js, terminal/desktopTerminalManager.js and utils/WebsocketHelper.js); ws ^8.18.0.
- Data: yaml 2.9.0; stream-json 1.9.1; lodash-es ^4.17.21.

Runtime requires seen in dist/ (number of files requiring each): electron 99; node:path 78; node:fs 68; node:crypto 37; node:os 22; node:child_process 20; @next-agent/browser-runtime 17; node:http 14; electron-log 12; sc-prepare-next 11; zod 9; node:url 8; node:stream 8; node:net 7; node:assert 7; node:util 6; @next-agent/screen-live-service 6; @modelcontextprotocol/sdk 6; node:events 5; node:https 4; socket.io-client 3; node:sqlite 2 (only the browserDataImport readers); vitest 1 (a shipped test file, shortcutService/modifierOnlyShortcut.test.js); yaml, tldts, stream-json, semver, qrcode-generator, node-pty, lodash-es, electron-window-state, electron-updater, compare-versions, @steipete/sweet-cookie and @beeper/webauthn-authenticator (1 each). [SRC]

Transitive packages present in node_modules (260 top level). Consumers are given only where verified:
- LiveKit and WebRTC: livekit-client (consumed by screen-live-service), @livekit/protocol, @livekit/mutex, webrtc-adapter, sdp, sdp-transform. [SRC]
- Terminal UI, used by the Stripe Link CLI: ink, ink-spinner, react, react-reconciler, yoga-layout, incur, yargs, cliui, chalk, boxen. Verified consumers: @stripe/link-cli, incur, ink, ink-spinner, react-reconciler. [SRC]
- MCP transport: hono and @hono/node-server (declared by the MCP SDK and the node-server package). [SRC package.json]
- Protobuf and Connect: @bufbuild/protobuf (wire format, see section 5). [SRC]
- Unverified consumers (INF, low confidence): @toon-format/toon, tokenx, mppx, viem, ox, abitype, @noble/*, @scure/*, jose, pkce-challenge, structured-headers, machina, rxjs, ky, @scalar/* (API reference UI), @stripe/stripe-js, @pnpm/* (npm config readers), @cfworker/json-schema (MCP SDK validator), update-notifier, latest-version, package-json.

## 2. Directory and module map

Package root: extract/manus/
- package.json: manifest above.
- dist/: main process (231 files). Subsystems below.
- frontend/out/: renderer static export (Next.js).
- assets/: app icons; assets/browser/ holds browser icons.
- node_modules/: runtime packages plus the two @next-agent workspace packages.

dist/ top level (entry and glue):
- index.js (main entry), preload.js (renderer bridge), env.js (prodEnv and devEnv hosts), generated-env.js (BUILD_ENV, BUILD_DISTRIBUTION), consts.js (macOS settings URLs, ports), setupIPC.js.
- setupMenu.js, desktopTray.js, setupDeepLinks.js, setupAppProtocol.js, setupAddonProtocol.js, setupLocalMediaProtocol.js, setupNavigationHandlers.js.
- themeService.js, quitConfirmationService.js, linuxUpdateSupport.js, autoUpdaterManager.js, desktopVersionTestManager.js.
- authService.js, desktopLoginToken.js, connectorCompletion.js, connectivityMonitorService.js, desktopShortcutLinkManager.js, localFileService.js, localMediaUrls.js.
- screenLiveService.js, screenLiveController.js, configureUserData.js.

dist/ipc/ (IPC layer):
- registry.js: registerIpc(), allHandlers (26 namespaces), allEvents (9), allScopedEvents (14), getExposedMeta().
- constants.js: channel names desktop-ipc-api, desktop-ipc-event, desktop-ipc-meta.
- handlers/ (29 files, 26 of them registered): addonLocal, addonWorkspace, appearance, auth, browser, browserDataImport, capture, computerUse, connectorCompletion, debugTools, desktopActivityIndicator, desktopOperator, env, globalVoiceInput, keepAwake, localMcp, presenter, remoteControl, screenLive, shortcut, skillRecording, system, terminal, theme, updater, window. Also githubPullRequestPreview (not registered), menuLabels (not registered), SystemHandlerTypes (types only).
- events/ (20 files): addonLocal, appearance, auth, browser, capture, desktopActivityIndicator, desktopOperator, keepAwake, menu, myComputer, navigation, remoteControl, screenLive, shortcut, skillRecording, system, terminal, theme, updater, window.
- browserIpc.js (browser handler and event tables), emitScopedEvent.js. Method lists per namespace are in Appendix A.

dist/windowManager/ (windows):
- WindowManager.js (singletons, createWindow, openPresenterMode), index.js, types.js, windowConfig.js, windowUrlHelper.js (app://manus in prod; localhost in dev), windowZoom.js.
- windows/: BaseWindow, MainWindow, PresenterWindow (fullscreen, route /presenter), ScreenLiveViewerWindow (route /screen-live-viewer), DesktopActivityIndicatorWindow (route /desktop-activity-indicator), TaskWindow, TaskDragPreviewWindow.

dist/agentManager/ (local operator):
- operatorManager.js (spawns manus-computer-operator.exe, lifecycle, env whitelist, orphan cleanup, sandbox setup, device-token exchange), operatorLocalApi.js (named-pipe client), operatorLocalApiUtils.js, operatorManagerUtils.js, workspaceFileWatcher.js, worktreeRetentionProtocol.js.

dist/computerUse/ (computer-use helper, skill recording, operator MCP config):
- AddonHelperLocator.js, addonHost.js (CodesignVerifier), addonSigningIdentity.js, autoWorkspaceManager.js, computerUseHelperProcessManager.js, deviceAddonLocators.js, helperTermination.js, linuxPortalRestoreTokenStore.js, operatorMcpConfig.js, operatorMcpTypes.js, remoteSkillDeliveryService.js, remoteSkillRecordingService.js, skillRecordingAccountService.js, skillRecordingClient.js, skillRecordingRpc.cjs (generated Connect/protobuf-style codec), skillRecordingRpc.js, skillRecordingRpcSmoke.js.

dist/addonLocal/ (addon runtime on the desktop):
- desktopLocalAddonService.js (main service; log redaction at lines 91-94), addonAccessService.js, addonJsonStorage.js, addonLocalArtifactStore.js, addonProtocolIdentifiers.js, deviceServiceWidgetRoutes.js, localAddonActivationOperation.js, pendingDeepLinkAction.js, webdevPreviewService/ (WebdevPreviewService, webdevPreviewFrameScripts.js, WebdevPreviewTypes.js).

dist/addonBrowserWorker/ (sandboxed worker for addon browser frames):
- addonBrowserWorkerService.js (net endpoint, sandboxed webPreferences), addonBrowserWorkerPolicy.js, addonBrowserWorkerLoopback.js, addonBrowserWorkerFileResponse.js, plus two Smoke harnesses.

dist/browser/ (in-app browser, 29 files):
- BrowserDriver.js (main-process driver; rewrites http: URLs to https://manus.im paths), BrowserMcpRuntime.js (MANUS_BROWSER_PARTITION), BrowserMcpServer.js, browserInspect.js, browserNativeShortcutState.js, browserPdfPageState.js, browserPdfViewerUi.js, browserPipState.js, setupBrowserWebview.js, tabPreload.js (exposes __manusBrowserPasskeys).
- passkey/ (BrowserPasskeyManager, BrowserPasskeyLifecycle, browserPasskeyModal, browserPasskeyChannels, browserPasskeyUtility); passwordManager/ (BrowserPasswordManager, savePromptPreload.js exposes manusPasswordPrompt, passwordSelectionPreload.js); 11 *Smoke.js harnesses.

dist/browserDataImport/ (import from installed browsers; sensitive):
- BrowserDataImportManager.js, BrowserProfileRegistry.js, BrowserPasswordVault.js, ChromiumPasswordReader.js (node:sqlite), FirefoxPasswordReader.js, SweetCookieReader.js, WindowsChromiumAppBoundCookieReader.js (node:sqlite), browserImportHelper.js (helper ManusBrowserImport.exe), browserDataImportTypes.js.

dist/myComputerService/ (the desktop as a remote device):
- myComputerService.js, SessionSupervisor.js, sidecarManager.js (spawns sidecar.exe), socketClient.js (socket.io, WebSocket transport only), types.js.

Other dist/ modules:
- terminal/: LocalTerminalManager.js, desktopTerminalManager.js, terminalConsts.js.
- captureService/: captureService.js, captureLaunchProtocol.js, types.js.
- desktopActivityIndicator/: desktopActivityIndicatorService.js, activityIndicatorMaterial.js, desktopActivityIndicatorTypes.js.
- globalVoiceInput/: globalVoiceInputService.js, types.js.
- keepAwake/: keepAwakeService.js, keepAwakeTypes.js.
- shortcutService/: shortcutService.js, shortcutConfig.js, modifierOnlyShortcut.js (+ test), modifierOnlyShortcutMonitor.js, windowShortcutState.js, types.js.
- systemNotification/: systemNotificationService.js, notificationAuthorizationBinding.js.
- fileGrant/: fileGrantService.js, localFileHandoff.js.
- remoteControl/: remoteControlUiHintService.js, RemoteControlUiHintState.js.
- utilityHost/ (utility process): utilityHostService.js (net endpoint; CDN allowlist for mermaid, highlight.js and katex), emailHost.js (Gmail and Graph APIs), emailDownloadHost.js, utilityScheduleBackend.js, utilityHostSmoke.js.
- utils/ (22 files): envHelper.js, envOverrideStorage.js, cspHelper.js, corsHelper.js, trustedUrls.js, deviceCredentialStore.js, linuxDeviceIdRecoveryStore.js, storageHelper.js (userData localStorage.json, device-id derivation, token cookie), loggerHelper.js, devToolsHelper.js, webdevProjectManifest.js, windowsPowerShell.js, WebsocketHelper.js, StepTimerHelper.js, EventEmitter.js, commonUtils.js, contextMenu.js, fileMime.js, byteRange.js, localPathHelper.js, macApplicationIconScript.js, preloadPath.js.

install resources/ (not in the extracted copy):
- app.asar (262 MB; the packaged dist/, frontend/out/ and node_modules), app.asar.unpacked/node_modules (node-pty, @next-agent native executables).
- bin/: manus-computer-operator.exe, manus-utilities.exe, sidecar.exe, ManusBrowserImport.exe (410 KB), build.json.
- addons/manus-computer-use/1.0.25/ (addon.json, helper/win32-x64 and win32-arm64 ManusComputerUseHelper.exe, runtime/computer-use-js.mjs at 101,901 bytes, runtime/node_modules with quickjs-emscripten).
- addons/skill-recorder/1.0.2/ (addon.json; the helper is shared with manus-computer-use).
- managed-addons/ (8 tar.gz archives and manifest.json; see 3.5 and Appendix C).
- browser-js/in-app-browser-js.mjs (53,096 bytes) with its own node_modules.
- locales/ (55 Chromium .pak), resources.pak, icudtl.dat, ffmpeg.dll, libEGL, libGLESv2, vk_swiftshader, dxcompiler, d3dcompiler (Electron runtime).

## 3. Feature candidates (grouped by taxonomy)

Each bullet names the capability, its pointer, and its provenance. Bullets are the only lines in this section that start with "- ".

### 3.1 Chat experience (the desktop shell; the chat UI itself lives in the renderer bundle)

- Main window loads the renderer from app://manus in prod or http://localhost:PORT in dev; route /app when signed in and /login otherwise (MainWindow.js); task windows load /app/SESSION_ID (TaskWindow.js); served by the app protocol. [SRC dist/windowManager/windowUrlHelper.js, dist/setupAppProtocol.js, frontend/out/app.html]
- Application menu items: New Tab, New Task, New Project, New Scheduled Task, Find, Settings..., Check for Updates..., Download Mobile App, Close, plus File, Edit, View. Each emits a menu event (onNewTask, onNewProject, onNewScheduledTask, onFindInPage, onSettingsRequested, onCheckForUpdates, onDownloadMobileApp). [SRC dist/setupMenu.js, dist/ipc/events/menu.js]
- Deep links into the chat: manus://app navigates to a session (onAppDeepLink, onNavigateToSession, onOpenBrowserSessionLink); manus://settings/... opens a settings route (onOpenSettingsRoute). [SRC dist/setupDeepLinks.js, dist/ipc/events/navigation.js]
- Task windows: a task can open in its own window (window.openTaskInNewWindow), be pinned (get/setTaskWindowPinned), and be dragged out with a preview window (startTaskDragPreview, stopTaskDragPreview). [SRC dist/ipc/handlers/window.js, dist/windowManager/windows/TaskWindow.js, TaskDragPreviewWindow.js]
- Floating desktop activity indicator (overlay window, route /desktop-activity-indicator) shows task state and offers stop, dismiss, mark-read, open, drag, expand and mouse-interaction toggles (handler methods updateSnapshot, notifyTask, openTask, markTaskRead, dismissTask, stopTask, setExpanded, setMouseInteractionEnabled, startDragging, dragTo, stopDragging, showContextMenu). [SRC dist/ipc/handlers/desktopActivityIndicator.js, dist/desktopActivityIndicator/*]
- Presenter mode: separate fullscreen window at route /presenter, fed by a data handoff (presenter.open, presenter.ready, presenter.getData). The code does not state the purpose; slide or presentation view is INF, low-medium confidence. [SRC dist/ipc/handlers/presenter.js, dist/windowManager/windows/PresenterWindow.js]
- Renderer keyword hits (substring counts over chunks, not feature proof): workspace 1,038; workspace members 25, workspace presets 25, workspace admin 48; wide research 108; research (generic) 531; artifact 311 and artifacts 117; citation 85 and citations 27; regenerate 346; fork 258; share link 143 and shared link 48; shared project 26; knowledge base 198; checkpoint 246 and checkpoints 93; rewind 73; thinking 88; reasoning effort 150; subagent 187; transcripts 75; custom instructions 67. Canvas 2,999 is ambiguous and includes the HTML canvas API. [SRC keyword scan, frontend/out/_next/static]
- Export and format choice: renderer i18n strings include an output-format chooser (Czech locale string seen; wording not reproduced). Export is generic (5,746 hits), so the scope is unclear. [SRC renderer chunks, low confidence]

### 3.2 Multimodal and creation

- Screen capture to chat ("send to Manus"): capture handler methods getScreenRecordingPermission, requestScreenRecordingPermission and consumePendingSendToManus; event capture.onSendToManus; capture process launched through captureLaunchProtocol.js. [SRC dist/ipc/handlers/capture.js, dist/captureService/*]
- Capture shortcut defined but disabled by default and unassigned: capture-screenshot (accelerator empty, enabled false). [SRC dist/shortcutService/shortcutConfig.js]
- Copy a page region as an image: system.copyPageRegionAsImage. [SRC dist/ipc/handlers/system.js]
- Global voice input (INF from method names: system-wide dictation that inserts text into a target app): globalVoiceInput handler methods beginTarget, checkTargetAccess, getMicrophoneConfig, setMicrophone, insertText, clearTarget, consumePendingAction, setPhase, setAudioLevel, showMessage; default shortcut global-voice-input disabled and unassigned. [SRC dist/ipc/handlers/globalVoiceInput.js, dist/globalVoiceInput/globalVoiceInputService.js, dist/shortcutService/shortcutConfig.js]
- Speech and transcription: Go package manus-utilities pkg/speechtotext (binary strings); skill-recording voice segments via skillRecording.readVoiceSegment and completeTranscription; renderer hits voice input 107, speech 102, transcribe 46, transcription 44. [SRC binary strings; SRC dist/ipc/handlers/skillRecording.js; keyword scan]
- Text-to-speech or voice output: no desktop-side evidence found (no tts strings in dist; not searched in the renderer). [INF]
- File upload and drop: system.showFileDialog and showFolderDialog; addonLocal.pickFrameFiles and grantFrameDroppedFiles; desktopSync.getPathForFile (Electron webUtils, preload.js); system.readClipboardFilePaths; fileGrant service. [SRC]
- Local media preview: local-media:// scheme (localMediaUrls.js, setupLocalMediaProtocol.js); system.getLocalMediaPreviewUrl, readLocalFileForPreview, readFileAsArrayBuffer. [SRC]
- Slides (managed addon manus-slides 0.0.1-prod): browser/slides-renderer (index.html, measurement.js, worker.js), runtime/mcp/deviceServer.mjs, skill docs html-syntax.md and pptx-xml-syntax.md. The addon renders HTML to PPTX-style output. [SRC tar listing; DOC SKILL.md names]
- Office and spreadsheet keyword hits in the renderer: slides 2,621; pptx 416; docx 204; xlsx 231; spreadsheet 853. [SRC keyword scan]
- Video editing (managed addon manus-video-editor 0.1.0-prod): bin/ffmpeg.exe, dist/core, dist/editor, dist/headless, dist/diagnostics, dist/browser-runtime. Renderer hits: video editor 359; the renderer has a video-entry route whose purpose is not in dist. [SRC tar listing, renderer routes]
- Image generation and editing: renderer hits image generation 151; no desktop-side implementation found. [SRC keyword; INF]
- Charts and diagrams: renderer hits mermaid 816 and katex 1,115. The utility host allowlists CDN loads of mermaid@10.6.1, highlight.js 11.9.0 and katex@0.16.25 (jsdelivr). [SRC dist/utilityHost/utilityHostService.js lines 35-40]
- Trip planning (managed addon manus-trip 0.0.1-prod): runtime/mcp/server.mjs; skills hotel-search.md, hotel-query-examples.md, display.md. Renderer hits hotel 144. [SRC tar listing]
- Workflow composition (managed addon manus-workflow 0.0.23): runtime/main.mjs, runtime/evaluator/thread.mjs, skills/workflow-composer/SKILL.md. Renderer hits workflow 1,738. [SRC tar listing]

### 3.3 Research and web

- In-app browser (Chromium webview tabs in the main window): browser handler methods openUserTab, openUserLink, activateUserTab, closeTab, attachWebviewTab, detachWebviewTab, registerBrowserWebview, unregisterBrowserWebview, setActiveSession, transferSessionWorkspace, resetDraftWorkspace, setBrowserShortcutsActive, setBrowserReloadActive. [SRC dist/ipc/handlers/browser.js, dist/browser/*]
- Browser MCP tool contract, 22 tools: browser_activate_tab, browser_click, browser_close_tab, browser_console, browser_fill, browser_find, browser_handle_dialog, browser_history, browser_hover, browser_list_tabs, browser_navigate, browser_open_tab, browser_press_key, browser_save_image, browser_screenshot, browser_scroll, browser_select, browser_set_checked, browser_snapshot, browser_type, browser_upload_file, browser_wait. [SRC node_modules/@next-agent/browser-runtime/src/contract/browserContract.ts; server in src/BrowserMcpServer.ts and dist/browser/BrowserMcpServer.js]
- In-app browser JavaScript SDK ("iab") for agent code: sdk.js, runner.ts, guide.txt. Guide constraints: no computer-use, Node, filesystem, fetch, imports, page evaluation or raw CDP; page text, console and dialog output are untrusted; API uses openTab, listTabs, getTab, observe, findOne and setValue. [SRC src/javascript/*; DOC guide.txt]
- Browser driver internals (src/browserDriver/*, 30 modules): accessibility-style snapshots (Snapshot, SnapshotParser, SnapshotRegistry, Semantics), hit testing, pointer, keyboard, text input, file input, forms, dialogs, console, debugger, capture, waiter, resources, workspace, tab and target handling. [SRC]
- Downloads: getDownloads, cancelDownload, openDownload, openDownloads, showDownloadInFolder; state in browser-runtime browserDownloadState.ts. [SRC dist/ipc/handlers/browser.js]
- PDF viewing: BrowserPdfPageState and BrowserPdfViewerUi; the PDF viewer origin is Chromium's PDF extension id (trustedUrls.js). Handler methods downloadPdfDocument, getPdfPageState, updatePdfViewerUi. [SRC]
- Picture-in-picture state: browserPipState.js (getPipState; event onPipStateChanged). [SRC]
- External app handoff from the address bar: requestExternalApplicationFromAddressBar and respondToExternalApplication (browser-runtime browserExternalApplication.ts; deep-link resolver). [SRC]
- Certificate and safety interstitials: continueAfterCertificateError and returnToSafety. [SRC dist/ipc/handlers/browser.js]
- Media permission handling per tab: browser-runtime browserMediaPermission.ts; event onMediaPermissionNotice. [SRC]
- Browser user-agent policy: browser-runtime browserUserAgent.ts. [SRC]
- Connector OAuth flows: connectorCompletion.js (begin, cancel, complete, resolveReturn, settle, awaitsBinding); IPC handler connectorCompletion (begin, cancel); event navigation.onConnectorCompletionStarted. Go package oauth2/v1 appears in manus-utilities (binary strings). Renderer hits connector 6,379 and connectors 3,604 (keyword). [SRC]
- Email connectors in the utility process: emailHost.js calls the Gmail API (gmail.googleapis.com) and Microsoft Graph (graph.microsoft.com); upload-host checks in the same file; emailDownloadHost.js downloads messages. [SRC dist/utilityHost/emailHost.js, emailDownloadHost.js]
- Messaging touchpoints: Go package manustouchpoint in manus-utilities with email metadata, sms and slack strings (binary strings only). Product scope is INF, low confidence. [SRC binary strings]
- Deep research: the string "deep research" has zero renderer hits; "wide research" has 108. The desktop shows no deep-research mode of its own. [SRC keyword; INF]

### 3.4 Agent runtime

- Local operator process: agentManager/operatorManager.js spawns manus-computer-operator.exe (constant OPERATOR_BINARY_NAME). Lifecycle: init, startProcess, stopProcess, restart, scheduleRestart with backoff (OPERATOR_RESTART_DELAY_MS, OPERATOR_RESTART_MAX_DELAY_MS, OPERATOR_RESTART_RESET_MS), serialized queue (enqueue), orphan removal, operator.log with a size cap (OPERATOR_LOG_MAX_BYTES). [SRC dist/agentManager/operatorManager.js]
- Operator environment whitelist: ENV_WHITELIST, ENV_WHITELIST_WINDOWS, ENV_WHITELIST_LINUX (operatorManager.js). [SRC]
- Operator restarts on login change and recorder context change: handleLoginTokenChanged, restartForRecorderContext. [SRC operatorManager.js]
- Operator device tokens: exchangeDeviceToken and revokeDeviceToken. [SRC operatorManager.js]
- Operator local API over a Windows named pipe (JSON messages with method and params; notifications; buffered-bytes cap MAX_BUFFERED_BYTES; connect backoff constants). Methods: file.roots, file.locations, file.list, file.search, workspace.resolve, operator.activity_status, operator.connection_status, operator.begin_maintenance_v2, operator.cancel_maintenance, worktree.retention.get, worktree.retention.list, worktree.retention.set, worktree.retention.preview, worktree.retention.prepare, worktree.retention.cleanup, addon.local_preview.resolve, host.ui_hint; notification remote_control_requests_changed. [SRC dist/agentManager/operatorLocalApi.js]
- Endpoint keys recovered by the REA trace on operatorManager.js (namespace "network"): addon.runtime.resolve, addon.list, addon.local.sync, addon.local.remove, addon.device_service.acquire, addon.device_service.renew, addon.device_service.release, addon.device_service.connect, addon.device_service.resolve, addon.device_service.grant_files, workspace.binding_status, desktopOperatorEnabled. [REA endpoint nodes]
- desktopOperator IPC namespace: watchWorkspaceFiles, unwatchWorkspaceFiles, getState, setEnabled, resolveSessionWorkspace, getWorktreeRetention, listWorktreeRetention, setWorktreeRetention, previewWorktreeCleanup, prepareSelectedWorktreeCleanup, cleanupWorktrees, listWorkspaceLocations, listWorkspaceDirectory, searchWorkspaceFiles, searchWorkspaceFilesByAbsolutePath; events onWorkspaceFilesChanged. [SRC dist/ipc/handlers/desktopOperator.js]
- Feature flags read in code: desktopOperatorEnabled, desktopActivityIndicatorEnabled, keepComputerAwakeEnabled. The server-side source of these flags is not visible. [SRC]
- Background-task awareness for updates: updater.setTaskRunning and setAutoInstallWhenTaskIdle, with consumeInstalledUpdateVersion; autoUpdaterManager.js defers install while tasks run. [SRC dist/ipc/handlers/updater.js, dist/autoUpdaterManager.js]
- Keep-awake while work runs: keepAwake.getState and setEnabled; event onStateChanged; dist/keepAwake/keepAwakeService.js. [SRC]
- Scheduling: menu item New Scheduled Task; utilityHost/utilityScheduleBackend.js (schedule write outcome handling); managed addon manus-triggers (runtime/mcp/server.mjs; skill references builtin-tools.json and connector-runtime.md); Go packages schedule/v1 and session.Schedule in manus-utilities (binary strings). [SRC]
- Worktree retention and cleanup, the closest desktop analogue to checkpoints and rewind: worktreeRetentionProtocol.js (settings, inventory, preview, prepare, cleanup request and result). [SRC dist/agentManager/worktreeRetentionProtocol.js]
- Error recovery and resilience: operator restart backoff; connectivityMonitorService.js (notifyNetworkOnline and notifyNetworkOffline); terminal session recovery (listRecoverable, listRecoverableSessionLocators); storage recovery commands (storageHelper.js buildRecoveryReadCommand, buildRecoveryWriteCommand, classifyRecoveryExit); device credential recovery store (utils/deviceCredentialStore.js); linuxDeviceIdRecoveryStore.js. [SRC]
- Sidecar supervision for My Computer sessions: SessionSupervisor.js calls sidecarManager.spawn(folderPath, sandboxWsUrl). [SRC dist/myComputerService/SessionSupervisor.js]
- Subagents and parallel work: no desktop-side symbols found (subagent count 0 in dist). Renderer hits subagent 187 and wide research 108. [SRC keyword; INF]
- Context compaction and long-run memory: no desktop-side evidence. [INF]
- Hooks and evals: shipped smoke harnesses (see 3.10). [SRC]

### 3.5 Tools, plugins and extensibility

- Addon system (plugin-like): addonLocal IPC handler methods list, sync, activate, cancelAction, describeAction, reload, remove, removeWithReport, getJson, postJson, isEnabled, isWorkspaceBindingReady, openFrame, closeFrame, connectFrameDeviceService, pickFrameFiles, grantFrameDroppedFiles, frameStorage, stageLocalArtifact, resolveLocalPreview, openLocalScope, closeLocalScope, registerLocalFrameScript, unregisterLocalFrameScript, takePendingDeepLinkAction, acquireDeviceService, renewDeviceService, releaseDeviceService; event onCatalogChanged. [SRC dist/ipc/handlers/addonLocal.js, dist/addonLocal/*]
- Addon manifest format (addon.json, schemaVersion 1): platforms keyed by win32-x64 and win32-arm64; each has a helper path, an mcp block (server, description, transport stdio or http, args) and optionally sharedHelperAddonId. [SRC install resources/addons/*/addon.json]
- Managed bundled addons, manifest schema manus.desktop.bundled-addons/v1 (environment prod, target windows-amd64), 8 entries: manus-slides 0.0.1-prod.37468226838.d1a18268; manus-storage 0.1.5; manus-tasks 0.0.15; manus-triggers 0.0.58; manus-trip 0.0.1-prod.37279479499.7edf05fb; manus-video-editor 0.1.0-prod.37756172562.1; manus-webdev 0.0.68; manus-workflow 0.0.23. Each has a sha256 and size. [SRC install resources/managed-addons/manifest.json]
- Unsupported addon ids in the same manifest (catalog entries not bundled for this platform): manus-clue, manus-cronometer, manus-fitbit, manus-myfitnesspal, manus-oura, manus-plaid, manus-polar, manus-trainingpeaks, manus-withings. [SRC]
- Addon protocol: manus-addon://ADDON_ID/assets/... and /services/... (setupAddonProtocol.js; addonProtocolIdentifiers.js); WebSocket upgrade for services; CSP handling for text/html and image/svg+xml responses. [SRC dist/setupAddonProtocol.js]
- Addon device services: acquire, renew, release, connect, resolve, grant_files (endpoint keys above) and addon-facing file grants. [REA endpoint nodes; SRC]
- Computer-use addon (manus-computer-use 1.0.25): MCP server "computer-use" over stdio (--stdio). Its tool is "computer-use/js", which runs JavaScript that calls cu.<native_method>(...). cu members seen: cu.help, cu.budget, cu.getApp, cu.app, cu.phase, cu.get_app_state, cu.computer_use_finish. Native method names seen: computer_use_status, computer_use_finish, type_text, press_key, click_count, screenshot_id, screenshot_coordinates, request_permissions, capture_source. [SRC addon.json; SRC runtime/computer-use-js.mjs]
- Skill-recorder addon (1.0.2): MCP server "skill-recorder" over stdio (--stdio --server skill-recorder). Description: records a workflow demonstrated in local desktop apps and attaches it to the active Manus task. [SRC install addon.json]
- Managed addon MCP servers and skill files (tar listings, names only, nothing extracted): manus-storage (runtime/mcp.js, skill/SKILL.md); manus-tasks (runtime/server.mjs, skills/manus-tasks/SKILL.md); manus-triggers (runtime/mcp/server.mjs, runtime/skill/SKILL.md, references/builtin-tools.json, references/connector-runtime.md); manus-slides (runtime/mcp/deviceServer.mjs, runtime/skill/*); manus-webdev (runtime/diagnostic/manus-webdev-lsp.mjs with presets astro, css, dockerfile, gopls, graphql, html, json, markdown and more; checkpoint-preview.html); manus-workflow (runtime/main.mjs, runtime/evaluator/thread.mjs, skills/workflow-composer/SKILL.md); manus-trip (runtime/mcp/server.mjs, runtime/skill/*). [SRC tar listings]
- Local MCP server configuration for the operator: computerUse/operatorMcpConfig.js reads and writes mcp_servers.json in an operator-state folder under userData; transports stdio and http; socket path handling; skill-recorder entries. File contents were not read. [SRC]
- localMcp IPC handler (listStaleAutoWorkspacePaths, ensureAutoWorkspaceFolder) with computerUse/autoWorkspaceManager.js: auto-created workspace folders for MCP work. [SRC]
- MCP SDK in use: @modelcontextprotocol/sdk 1.29.0 (main dependency; BrowserMcpServer in browser-runtime; Go sdk-mcp-go client transport inside the operator binary, binary strings). [SRC]
- Deep-link extension entry points: manus://addon-local/install and manus://addon-local/remove; manus://app; manus://settings/...; manus://update-test. [SRC dist/setupDeepLinks.js]
- Keyboard shortcuts, user-configurable: toggle-main-window (Alt+M, enabled); capture-screenshot and global-voice-input (disabled, unassigned); modifier-only accelerators (Ctrl, Alt, Shift, Command, left and right variants) via modifierOnlyShortcut.js and modifierOnlyShortcutMonitor.js; window-scoped command state in windowShortcutState.js; shortcut IPC methods getConfig, getCapabilities, beginRecording, endRecording, setEnabled, update, reset, setWindowBindings. [SRC dist/shortcutService/*, dist/ipc/handlers/shortcut.js]
- CLIs: Go binaries manus-utilities.exe, manus-computer-operator.exe, sidecar.exe (repos manusCli and manusUtilities). Their user-facing command names were not recovered (see section 6). [SRC build.json; binary strings]
- Stripe Link CLI launched for the operator: env MANUS_OPERATOR_LINK_CLI set to the package's dist/cli.js (operatorManager.js near line 564; package @stripe/link-cli 0.23.1). [SRC]
- Renderer-to-main API for the app's own UI (the extension surface for a host): ipcMain.handle('desktop-ipc-api', (event, method, ...args)) dispatches namespace.method calls; senders must match the trusted-URL policy. [SRC dist/ipc/registry.js, dist/utils/trustedUrls.js]
- Renderer bridge exposed by preload.js (exposeInMainWorld): desktopSync (getDeviceId, getSystemLocale, getEnvState, getPathForFile); customStorage (synchronous storage get, set, remove, clear, key, length); authStorage (getToken). [SRC dist/preload.js]
- Marketplace or plugin store on the desktop: marketplace 0 and plugin 0 in dist. Renderer hits marketplace 460 and plugin 779 (keyword). [SRC]

### 3.6 Computer and environment control

- Computer-use helper: resources/addons/manus-computer-use/1.0.25/helper/win32-x64/ManusComputerUseHelper.exe (and win32-arm64), supervised by computerUse/computerUseHelperProcessManager.js, located through AddonHelperLocator.js and deviceAddonLocators.js, terminated by helperTermination.js. [SRC]
- Helper signature check: addonHost.js CodesignVerifier runs /usr/bin/codesign verify with a designated requirement on macOS; allowed identities in addonSigningIdentity.js. The Windows verification path is not visible in the code read. [SRC]
- Computer-use IPC: computerUse.listInstalledApps and computerUse.getAppIcon; system.getLocalFileApplications, system.addApplicationPath, system.openLocalPathWithApplication. [SRC dist/ipc/handlers/computerUse.js, system.js]
- Screen live (share the screen into a Manus session): screenLive IPC methods requestStart, getState, stop; event onStateChange; main-side services dist/screenLiveService.js and dist/screenLiveController.js. [SRC]
- Screen-live runtime package (node_modules/@next-agent/screen-live-service/src): electron/ (ScreenLiveRecorder, ScreenLiveRegionCache, ScreenLiveRustProcess, ScreenLiveRustRuntime, ScreenLiveRustStreams, Windows worker, Windows task, diagnostics, screenLiveBridgeServer); runtime/ (screenLiveCursor, ScreenLiveMoveCoalescer, ScreenLiveClipboardExporter, ScreenLiveClipboardReceiver, screenLiveRegionSelector). LiveKit is used by ScreenLiveClipboardReceiver.ts. [SRC]
- Native executables for screen live and remote input: dist/native/manus-screen-live-publisher.exe and manus-remote-input.exe (under app.asar.unpacked). The publisher is spawned by ScreenLiveRustProcess. [SRC]
- Remote input and clipboard protocol (src/remoteInput/*): remoteInputService, inputProtocol, clipboardProtocol, clipboardExportProtocol, cursorProtocol, ClipboardExportService, clipboardFileService. Message types seen: hello, offer, subscribe, unsubscribe, renew, ready, read_current, paste_text, paste_commit, paste_cancel, paste_files_commit, paste_result, files_hello, files_ready, cursor_state, error. [SRC]
- Screen-live viewer window (route /screen-live-viewer): getScreenLiveViewerData, fitScreenLiveViewer, finishScreenLiveViewerClose; close request event onScreenLiveViewerCloseRequested. [SRC dist/windowManager/windows/ScreenLiveViewerWindow.js, dist/ipc/handlers/window.js]
- Skill recording (demonstrated workflows): computerUse/skillRecordingClient.js; skillRecordingRpc.cjs (Connect-style envelope and protobuf codec); skillRecordingAccountService.js; remoteSkillRecordingService.js; screen-live-service exports ./recording and src/recording/skillRecordingProtocol.mts with message types artifact, cancel, completeTranscription, open, status, uploadArtifact. [SRC]
- Remote skill delivery to the account: remoteSkillDeliveryService.js; skillRecording IPC methods deliverRemote, takeRemoteDeliveries, completeRemoteDelivery, openRemoteTask, getRemoteAccountContext, authorizeRemoteDelivery, getAccountContext, authorizeDelivery, open, watch, status, readArtifact, cleanup, discard; event onRemoteDeliveryPending. [SRC]
- My Computer (INF from names, the sidecar and the sandbox WebSocket URL: the desktop as a remote device): myComputerService.js, socketClient.js (socket.io, WebSocket transport only), SessionSupervisor.js, sidecarManager.js (spawns sidecar.exe with a shared folder and sandbox WebSocket URL). Socket events seen: connect, connect_error, device_status, device_config, lifecycleChanged, error, exit, disconnect. Lifecycle states seen: connecting, connected, ready, offline, error. Event onLifecycleChanged. [SRC]
- Sidecar binary packages (binary strings): pkg/sidecar/terminal, pkg/sidecar/bridge, pkg/sidecar/fileops, pkg/sidecar/parentwatch, sdk/sandbox/v1, sdk/terminal/v1, sdk/bridge/v1 (sandboxv1.Request and DirEntry types). [SRC binary strings]
- Sandbox device concept: the computer-use addon description treats a device of type Sandbox with sessionType x11 as the agent's own desktop, to be operated without a takeover prompt; any other Linux desktop needs announced takeover and user consent. [SRC addon.json; DOC]
- Terminal sessions: terminal IPC methods listRecoverableSessionLocators, listRecoverable, create, transferSessionWorkspace, write, resize, attach, setActive, close; event terminal.onEvent. Implementation: dist/terminal/LocalTerminalManager.js and desktopTerminalManager.js (node-pty; socket.io-client), terminalConsts.js (MAX_TERMINALS, default columns and rows, poll intervals). [SRC]
- Terminal font asset: /terminal-font/JetBrainsMonoNerdFont-Regular.woff2 (renderer route). [SRC renderer]
- Local filesystem: system IPC methods readLocalFileForPreview, readFileAsArrayBuffer, writeLocalFile, watchLocalFile and unwatchLocalFile (event onLocalFileChanged), renameLocalFile, createLocalDocument, saveLocalFileBackup, saveLocalPathAs, openLocalPath, showLocalPathInFolder, showLocalPathContextMenu, getLocalPathInfo, readLocalMarkdownResource, resolveLocalFileLink; localFileService.js; fileGrant/fileGrantService.js; fileGrant/localFileHandoff.js. [SRC]
- Operator workspace filesystem: desktopOperator watchWorkspaceFiles and searchWorkspaceFiles, backed by operator file.* methods and agentManager/workspaceFileWatcher.js. [SRC]
- Git in the workspace: system.getWorkspaceGitInfo, getWorkspaceGitInfoResult, switchWorkspaceGitBranch (WorkspaceGitCommandError). [SRC dist/ipc/handlers/system.js]
- OS integration: system.openExternal; openFullDiskAccessSettings, openScreenCaptureSettings, openMediaAccessSettings, openNotificationSettings; system tray (desktopTray.js: Open Window, Quit); quit confirmation (quitConfirmationService.js; setQuitConfirmationLabels). [SRC]
- Notifications: systemNotification/systemNotificationService.js and notificationAuthorizationBinding.js; system.askNotificationPermission and checkNotificationPermission. [SRC]
- Microphone and screen-capture permissions: system.askMicrophonePermission, checkMicrophonePermission, checkScreenCapturePermission; capture.getScreenRecordingPermission and requestScreenRecordingPermission. [SRC]
- Windows safe mode and sandbox setup: desktopOperator.getWindowsSafeModeStatus and enableWindowsSafeModes call the operator binary's sandbox subcommand (operatorManager.js around lines 189-229). Linux sandboxing uses bwrap (operatorManager.js strings). The internals of "sandbox setup" were not read. [SRC]
- Payments via Stripe Link (INF: agent purchases): env MANUS_OPERATOR_LINK_CLI (see 3.5); Go packages in manus-utilities with types StripeLinkPayment, StripeLinkPaymentStatus, StripeLinkAuthorizationStatus, StripeLinkApproveStatus, StripeLinkPaymentBillSummary, StripeLinkPaymentBillDetail, StripeLinkPaymentBillLineItem, StripeLinkPaymentBillTotal (binary strings). Approval semantics are not verified. [SRC binary strings; INF]
- Browser data import from installed browsers (sensitive; paths only, no data read): browserDataImport/* with BrowserDataImportManager.importProfile and listProfiles; readers for Chromium passwords (node:sqlite), Firefox passwords, cookies (SweetCookieReader) and Windows Chromium app-bound cookies (node:sqlite); helper resources/bin/ManusBrowserImport.exe; result code administrator_approval_required. Renderer string: passwords and cookies can be chosen for import. [SRC]

### 3.7 Coding capability (desktop-side harness pieces)

- Workspace file watching and search: agentManager/workspaceFileWatcher.js; operator file.search and searchFilesByAbsolutePath; desktopOperator.searchWorkspaceFiles and searchWorkspaceFilesByAbsolutePath. No embeddings or code-index code found on the desktop. [SRC; INF on the absence]
- Git worktree lifecycle: worktreeRetentionProtocol.js (settings, inventory, preview, prepare, cleanup); desktopOperator worktree methods; operator worktree.retention.* methods. [SRC]
- Git branch and status: system.getWorkspaceGitInfo and switchWorkspaceGitBranch (see 3.6). Renderer keyword hits: worktree 877, pull request 298. [SRC]
- Pull-request and CI status preview: githubPullRequestPreview.js (GitHubPullRequestPreviewService: normalizeUrl, resolveExecutable, execute, parse, resolveStatus, hasFailedCheck, hasPendingCheck), using a resolved GitHub CLI executable; surfaced through system.getGitHubPullRequestPreview. [SRC dist/ipc/handlers/githubPullRequestPreview.js]
- Web development preview: addonLocal/webdevPreviewService/webdevPreviewService.js, webdevPreviewFrameScripts.js, WebdevPreviewTypes.js; utils/webdevProjectManifest.js; operator method addon.local_preview.resolve. [SRC]
- Web development addon (manus-webdev 0.0.68): runtime/checkpoint-preview.html and an LSP diagnostic runner (runtime/diagnostic/manus-webdev-lsp.mjs) with presets for astro, css, dockerfile, gopls, graphql, html, json, markdown and more. Archive has about 4,571 entries. [SRC tar listing]
- Local document creation and editing: system.createLocalDocument, renameLocalFile, writeLocalFile, saveLocalFileBackup. [SRC]
- Terminal as a coding surface: see 3.6 (node-pty sessions, recovery across restarts). [SRC]
- Renderer rendering of code and diagrams: highlight.js (CDN allowlist in utilityHost), mermaid and katex (see 3.2). The renderer's code editor is not identified (not searched). [SRC; blocker]
- Agent coding loops, test runs, and model-side code edit or apply: no desktop-side code found (INF). The desktop runs the operator; the loop itself is not visible here.

### 3.8 Memory, learning and personalization

- Skill learning from demonstration: skill-recorder addon plus skillRecording IPC; recordings can be delivered to the account and turned into skills. [SRC; see 3.6]
- Workflow composer skill (manus-workflow): skills/workflow-composer/SKILL.md. [SRC tar listing]
- Persistent memory, knowledge base, custom instructions: no desktop-side symbols (memory 0 files in dist). Renderer keyword hits: memory 1,036; knowledge base 198; custom instructions 67. [SRC keyword; INF]
- Renderer local storage bridge: customStorage in preload.js (sync IPC get, set, remove, clear, key, length); main-side storageHelper.js writes userData/localStorage.json (path only; contents not read). [SRC]
- Theme personalization: themeService.js (values system, light, dark); appearance.broadcastConfig and theme.broadcastConfig; events onConfigUpdated. [SRC]
- Locale: system.getSystemLocale and desktopSync.getSystemLocale; menuLabels handler (readMenuLabel, readMenuAnchor); renderer i18n JSON in chunks. [SRC]
- Device identity: desktopSync.getDeviceId; storageHelper.js derives a device-id name from a SHA-256 of the userData path (lines around 355-359). [SRC]

### 3.9 Models and routing

- No LLM provider SDK or model-name strings in the main process: openai, anthropic, gemini and deepseek each match 0 files in dist. No model picker or routing code found on the desktop. [SRC; INF]
- Renderer hints (keyword only): reasoning effort 150; thinking 88; credits 10,103 (billing or usage UI); usage 2,170. [SRC keyword; low confidence]
- Model-related environment: MANUS_OPERATOR_LINK_CLI only; operator environment whitelist. [SRC]
- Cost and usage meters on the desktop: none found (cost 0 files in dist). [SRC]

### 3.10 Transparency, observability and control

- Logs: electron-log (12 files); utils/loggerHelper.js; operator log pipes to operator.log with a size cap, getLogPath and openLogStream. [SRC]
- Developer tools: debugTools IPC (getState, unlock, openDevTools); utils/devToolsHelper.js. [SRC]
- Live operator and connection status: operator.activity_status and operator.connection_status over the operator API; desktop activity indicator state (getState, updateSnapshot). [SRC]
- Diagnostics modules: screen-live (ScreenLiveHostDiagnostics.ts, controlDiagnostics.ts, performanceDiagnostics.ts, lifecycleDiagnostics.ts, screenLiveWindowsDiagnostics.ts); browser (BrowserDiagnostics.ts; BrowserDriverActivity.ts, BrowserDriverConsole.ts, BrowserDriverDebugger.ts, BrowserDriverDialog.ts). [SRC]
- Browser console and history for agents: browser_console and browser_history tools (see 3.3). [SRC contract]
- Shipped test and smoke harnesses in the production dist: 11 files in browser/ (*Smoke.js, including browserMcpInteropSmoke.js), addonBrowserWorkerSmoke.js and addonBrowserWorkerServiceSmoke.js, computerUse/skillRecordingRpcSmoke.js, utilityHost/utilityHostSmoke.js, and shortcutService/modifierOnlyShortcut.test.js (vitest). The browser-runtime package also has a test:javascript script. [SRC]
- Telemetry: Sentry ingest hosts (sentry.dev.ops.butterfly-effect.dev and sentry.prod.ops.butterfly-effect.dev) appear in an addon CSP allowlist (setupAddonProtocol.js lines 30-31). No analytics SDK found (analytics 0 files). [SRC]
- Update test channel: desktopVersionTestManager.js with deep link manus://update-test and the policy-test event onPolicyTestChanged. [SRC]
- Preview-then-commit for destructive actions: worktree cleanup uses previewWorktreeCleanup and prepareSelectedWorktreeCleanup before cleanupWorktrees. [SRC]
- Activation audit trail for addons: localAddonActivationOperation.js writes activation reports and keeps operations for 30 days (OPERATION_RETENTION_MS). [SRC]
- Renderer audit wording: "audit log" 103 hits (keyword). [SRC keyword; low]

### 3.11 Safety, permissions and governance

- IPC sender trust: registerIpc() rejects calls whose sender URL is not trusted (utils/trustedUrls.js: app://manus, app://frontend, and localhost only in dev; a registry of trusted app documents). REA counts 7 sender-validation observations. [SRC; REA]
- Renderer bridge scope: preload.js exposes only desktopSync, customStorage and authStorage through safeExpose, which refuses to overwrite an existing global. [SRC dist/preload.js]
- Content security policy and CORS: utils/cspHelper.js (createDesktopCspHeader, createDesktopCspNonce, injectDesktopCspNonce); utils/corsHelper.js (setupCorsBypass); setupNavigationHandlers.js (isSafeFrameNavigationURL, isSafeAddonFrameNavigation); setupAddonProtocol.js rewrites CSP for text/html and image/svg+xml responses. [SRC]
- Browser hardening: setupBrowserWebview.js forces webPreferences.sandbox = true; the password prompt and passkey windows also use sandbox true. Media permissions are decided per tab (browserMediaPermission.ts). External app launch needs an explicit response (respondToExternalApplication). [SRC]
- File-access grants: fileGrant/fileGrantService.js (consumeFileGrant IPC); addonLocal grantFrameDroppedFiles and device-service grant_files; addonAccessService.js. [SRC; REA endpoint keys]
- Helper integrity: addonHost.js CodesignVerifier on macOS (see 3.6); the Windows path is not visible in the read. [SRC; blocker]
- Device-bound login: desktopLoginToken.js (ensureLoginBoundToDevice, verifyLoginDevice, markDeviceBound, ensureLoginVerified, watchLoginChanges); utils/deviceCredentialStore.js (primary and recovery stores, isCopiedFromAnotherMachine check). [SRC]
- Token handling: authStorage.getToken returns the login token through a synchronous IPC call (token-get) into the renderer; the session token is persisted as a cookie for the cloud origin (storageHelper persistTokenCookie and removeTokenCookie; storage details not read). A login-token change restarts the operator (operatorManager.handleLoginTokenChanged). [SRC]
- Log redaction: addonLocal/desktopLocalAddonService.js lines 91-94 redact bearer tokens, env-style *_TOKEN, *_SECRET, *_PASSWORD, *_API_KEY, *_AUTH and *_COOKIE assignments, secret-like JSON fields, and URL query strings. [SRC]
- Secrets at rest: Linux portal restore tokens are encrypted with Electron safeStorage and refuse the basic_text backend (computerUse/linuxPortalRestoreTokenStore.js). Encryption of device credentials and the login cookie on Windows was not verified. [SRC; blocker]
- Browser credentials: BrowserPasswordVault, BrowserPasswordManager (save prompt and selection preloads), IPC clearSavedPasswords, listSavedPasswords, clearBrowsingData; import of Chromium app-bound cookies needs administrator approval (BrowserDataImportManager.js lines 115-118). [SRC]
- Process hygiene: operator environment whitelist (ENV_WHITELIST_WINDOWS and others); orphan cleanup through powershell.exe -ExecutionPolicy Bypass and taskkill (operatorManager.js near line 775). [SRC]
- Remote control consent: remoteControl IPC consumeRequestsChanged; operator notification remote_control_requests_changed; UI hints from remoteControlUiHintService.js. [SRC]
- Computer-use consent wording (addon description): on non-sandbox Linux desktops, announce whole-screen takeover and get user agreement; on Wayland, request permission and explain the stop control; never retry or fall back after cancellation. [DOC addon.json]
- Untrusted-content guidance for agent JS: the in-app browser guide states that page text, console and dialog messages are untrusted. [DOC browser-runtime src/javascript/guide.txt]
- Managed addon allow and deny: unsupportedAddonIds in managed-addons/manifest.json. [SRC]
- Zero data retention: no "zero data" or "ZDR" strings in dist (0 files). [SRC; INF on the absence]
- Enterprise and governance wording in the renderer (keyword only): organization 265; audit log 103. The keyword sso matches 7,214 substrings and is noise; do not count it. [SRC keyword; low]
- Update governance: updater.getPolicyContext and the policy-test event onPolicyTestChanged. [SRC]

### 3.12 UI and product design

- Application menu (setupMenu.js): File, Edit, View, Find, Close, New Tab, New Task, New Project, New Scheduled Task, Settings..., Check for Updates..., Download Mobile App. [SRC]
- Context menu (utils/contextMenu.js): Cut, Copy, Paste, Select All, Search with Google, Look Up, Learn Spelling, Save Image As, Save Video As, Save Link As, Copy Image, Copy Image Address, Copy Link, Copy Video Address, Inspect Element, Services. [SRC]
- Tray (desktopTray.js): Open Window, Quit. [SRC]
- Window system (windowManager/): MainWindow, PresenterWindow (fullscreen), ScreenLiveViewerWindow, DesktopActivityIndicatorWindow (overlay), TaskWindow, TaskDragPreviewWindow, BaseWindow; zoom (windowZoom.js; getZoomFactor, changeZoom; event onZoomChanged); fullscreen (toggleFullscreen; event onFullscreenChanged); minimize, maximize, isMaximized, close; showAppMenu. [SRC]
- Shortcut defaults (shortcutConfig.js): toggle-main-window Alt+M enabled; capture-screenshot and global-voice-input disabled. [SRC]
- Theme (themeService.js): system, light and dark; native theme follow. [SRC]
- Onboarding and login: /login route; desktopLoginToken.js with QR support (qrcode-generator); login verification before device binding. [SRC]
- Activity indicator visuals: activityIndicatorMaterial.js (material surfaces; updateMaterialSurfaces and clearMaterialSurfaces). [SRC]
- Screen-live viewer controls: fitScreenLiveViewer and the viewer data handoff. [SRC]
- Platform-specific UI and packaging: linuxUpdateSupport.js (deb, rpm and AppImage update modes); MSIX packaging (AppxManifest.xml); macApplicationIconScript.js; Windows PowerShell helpers. [SRC]
- Internationalization: 55 Chromium locale .pak files; renderer translations in chunks (Czech strings observed); menu labels localized through the menuLabels handler. [SRC]
- Performance measures: ScreenLiveMoveCoalescer.ts (pointer-move coalescing), ScreenLiveRegionCache.ts, screen-live performance diagnostics; renderer has 1,414 static files and woff2 fonts. [SRC]
- Sounds route /sounds (purpose inferred as notification sounds, INF low). [SRC renderer tree]
- Accessibility: not verified by reading (no accessibility code search done). [blocker]

### 3.13 Platform architecture

- Processes: Electron main (dist/index.js); renderer windows on Chromium 150; one utility process (utilityHostService.js; REA reports 1 resolved utility entrypoint); addon browser worker (addonBrowserWorkerService.js, net endpoint, sandboxed BrowserWindow); operator child (manus-computer-operator.exe); sidecar (sidecar.exe); helpers (ManusComputerUseHelper.exe, ManusBrowserImport.exe, manus-screen-live-publisher.exe, manus-remote-input.exe); pty processes (node-pty). [SRC; REA]
- Main to renderer IPC: 'desktop-ipc-api' (request with method and args), 'desktop-ipc-event' (events), 'desktop-ipc-meta' (exposed names of handlers and events); synchronous IPC for storage, token and env reads (preload.js). [SRC dist/ipc/constants.js, registry.js, preload.js]
- Custom schemes: app:// (privileged scheme 'app', host manus, also frontend); manus-addon://; local-media://; deep links manus:// (prod) and manusdev:// (dev). [SRC]
- Operator IPC: Windows named pipe (operatorLocalApi.js connectNamedPipe and connectPipeOnce, PIPE_CONNECT_* backoff, MAX_BUFFERED_BYTES, JSON messages with method, params and notifications); a unix socket path is used on Linux and macOS (socketPath in operatorMcpConfig.js). [SRC]
- Wire formats: Connect protocol with protobuf (@bufbuild/protobuf; skillRecordingRpc.cjs, connectorCompletion.js, desktopLoginToken.js, addonAccessService.js, utilityHostService.js; "application/connect" strings). [SRC]
- My Computer and terminal transport: socket.io over WebSocket only (socketClient.js; desktopTerminalManager.js). [SRC]
- Chat and RPC endpoints from env.js: prod chatWebsocketUrl wss://api.manus.im and rpcServerUrl https://api.manus.im; dev chatWebsocketUrl wss://vida.butterfly-effect.dev and rpcServerUrl https://vidabiz.butterfly-effect.dev. The meaning of the "vida" names is not stated in the code (INF: dev or staging). [SRC env.js]
- Live media: LiveKit (livekit-client) for screen-live clipboard receiving; WebRTC libraries in node_modules; screen-live control messages validated with zod (inputProtocol.ts and others). [SRC]
- MCP over stdio and HTTP (operator config and addon.json transports). [SRC]
- Local storage in userData (paths only, not read): localStorage.json (renderer bridge), env-override.json, linux-portal-restore-tokens.v1.json (safeStorage-encrypted), device credential primary and recovery stores, operator state (mcp_servers.json under operator-state), per-addon JSON storage (addonJsonStorage.js), addon local artifact store, operator.log, browser partition (MANUS_BROWSER_PARTITION). External browser databases are read with node:sqlite only in the import readers. [SRC]
- Auth: session cookie for the cloud origin, login token exposed to renderer through authStorage, device-bound login verification, device tokens for the operator. PKCE library present (pkce-challenge in node_modules; consumer not verified). [SRC; INF]
- Distribution and update: electron-updater feeds (prod https://download.manus.im/, dev cloudfront feed); MSIX with runFullTrust; managed addons with a sha256 manifest; Go binaries with checksums in build.json; Linux package modes in linuxUpdateSupport.js. [SRC]
- Environment split: env.js (prodEnv and devEnv), envHelper.js (PROD_CONFIG and DEV_CONFIG, runtime override through envOverrideStorage), generated-env.js (BUILD_ENV, BUILD_DISTRIBUTION). [SRC]
- Rust-portable design notes (INF from the shipped design): the screen-live publisher and the remote-input helper are native helper processes that exchange zod-validated messages over IPC, a pattern that carries over to Rust; the operator named-pipe JSON API, the addon.json schema, the managed-addon manifest and the browser tool contract (22 tools) are candidate contracts to mirror; the Electron main process itself is JavaScript and has no Rust counterpart here.

## 4. Extension points

- Local MCP servers for the operator: mcp_servers.json in operator-state (stdio or http transports; computerUse/operatorMcpConfig.js). File contents not read. [SRC]
- Addon packages: addon.json (schemaVersion 1, per-platform helper and mcp block); managed bundles described by manifest.json (schema manus.desktop.bundled-addons/v1, sha256 per archive); addon assets and services over manus-addon://. [SRC]
- Addon frame scripts: addonLocal registerLocalFrameScript and unregisterLocalFrameScript; addon device services (acquire, renew, release, connect, resolve, grant_files). [SRC; REA endpoint keys]
- Skill files: SKILL.md with reference documents inside addon archives (workflow-composer, manus-tasks, manus-triggers, manus-slides, manus-trip, manus-storage) and the skill-recorder and computer-use addons. [SRC tar listings; DOC]
- Browser tools (22 browser_* tools) and the in-app browser JavaScript SDK for agent code. [SRC]
- Computer-use JavaScript tool (computer-use/js) with the cu.* API. [SRC addon.json; SRC runtime/computer-use-js.mjs]
- Deep links: manus://addon-local/install and /remove; manus://app; manus://settings/...; manus://update-test; dev variant manusdev://. [SRC]
- Operator named-pipe API (method namespaces file.*, workspace.*, operator.*, worktree.retention.*, addon.*). [SRC; REA endpoint keys]
- Renderer-facing IPC: 'desktop-ipc-api' with 26 namespaces and 20 event modules (Appendix A). [SRC]
- Shortcut accelerators (toggle-main-window, capture-screenshot, global-voice-input) and modifier-only shortcuts. [SRC]
- Go CLIs: manus-utilities.exe, manus-computer-operator.exe and sidecar.exe (command surface not recovered). Stripe Link CLI (npm package @stripe/link-cli) invoked by the operator. [SRC]
- Webhooks: none found in dist. [INF from absence]
- Feature flags: desktopOperatorEnabled, desktopActivityIndicatorEnabled, keepComputerAwakeEnabled. Their server-side source is not visible. [SRC]
- Environment override: envOverrideStorage (env-override.json, dev builds) and the setOverrideAndRelaunch handler. [SRC]
- Marketplace or plugin store on the desktop: none found (marketplace 0 and plugin 0 in dist). [SRC]

## 5. Data and protocol hints

- Storage (paths only, nothing read): userData holds localStorage.json (renderer storage bridge), env-override.json, linux-portal-restore-tokens.v1.json (safeStorage-encrypted), device credential primary and recovery stores, operator state including mcp_servers.json, operator.log, per-addon JSON storage and addon artifact store. node:sqlite is used only to read external browser databases during import. [SRC]
- Wire formats: Connect protocol with protobuf (@bufbuild/protobuf); socket.io over WebSocket for My Computer and terminal; a JSON request and notification protocol over a Windows named pipe (unix socket on Linux and macOS) for the operator; LiveKit WebRTC for screen-live clipboard; zod-validated messages for screen-live input and clipboard; MCP over stdio and HTTP. [SRC]
- Endpoints (env.js): prod chat WebSocket wss://api.manus.im and RPC https://api.manus.im; dev wss://vida.butterfly-effect.dev and https://vidabiz.butterfly-effect.dev. Other hosts seen: download.manus.im and a cloudfront update feed; cdn.jsdelivr.net for three render libraries; gmail.googleapis.com and graph.microsoft.com for email connectors; Sentry ingest hosts in an addon CSP allowlist; manus.im. [SRC]
- Auth: session cookie for the cloud origin (storageHelper persistTokenCookie and removeTokenCookie); login token exposed to the renderer via authStorage.getToken; device-bound login verification (desktopLoginToken.js); device token exchange and revocation for the operator. [SRC]
- Sync: My Computer lifecycle over socket.io (connecting, connected, ready, offline, error); the renderer-to-main sync bridge for storage and env; operator connection and activity status; workspace file watchers. [SRC]
- Recording and delivery: skill recordings are uploaded as artifacts (uploadArtifact) and delivered to the account (remote delivery; authorizeRemoteDelivery). [SRC]

## 6. Blockers and limits

1. REA graph is partial. Its own limitations report 201 "resource constraint: expanded application relationship" limits. The channel, string, route and API traces returned no match, which REA says is not evidence of absence. Only the operatorManager module trace matched (836 nodes). IPC and route claims in this file therefore rest on SRC reads.
2. Main-process TypeScript source is not shipped. package.json exports point to backend/ipc/registry.ts and backend/preload.ts, which are absent. Only compiled dist/ JS is present, with no source maps (0 .map files). Behaviour is read from the JavaScript.
3. The renderer (frontend/out, 124 MB) is a minified Next.js static export. No component source is present. UI features are inferred from keyword scans (substring counts, low precision) and from the IPC methods the renderer calls. The renderer was not analyzed by REA, to respect the one-heavy-analysis rule and the bundle size.
4. Native binaries (Manus.exe 239 MB, manus-computer-operator.exe, manus-utilities.exe, sidecar.exe, ManusComputerUseHelper.exe, ManusBrowserImport.exe, manus-screen-live-publisher.exe, manus-remote-input.exe) were checked only by string or symbol grep. No REA native analysis was run. The Go CLI command surface is not recovered.
5. Managed addon archives were listed, not extracted (rule: do not extract into the install area; archives treated as untrusted data). manus-webdev (about 4,571 entries) and manus-video-editor (about 3,230 entries) were not inventoried beyond their top-level directories. SKILL.md and MCP server contents were not read.
6. Computer-use internals are not read beyond addon.json and the cu.* names in runtime/computer-use-js.mjs (101,901 bytes). The QuickJS sandbox and helper protocol are not analyzed.
7. Windows-side helper signature verification is not visible; only the macOS codesign path was read (addonHost.js). Windows sandbox setup internals (operator sandbox setup) were not read beyond call sites.
8. Local data stores (userData, browser profiles, device credentials, login cookie, localStorage.json, operator-state) were referenced in code but never opened. No secret-bearing file was read or printed. The browser-import readers and the password vault are described from code only.
9. Accessibility, i18n coverage and performance were not verified by reading.
10. Some transitive packages have no verified consumer (see section 1). Names such as "vida" and the purpose of presenter mode and video-entry are inferred, not confirmed.
11. Renderer keyword counts are substring hits. Examples: sso 7,214 (noise), canvas 2,999 (includes HTML canvas API), segment in dist (voice segments and noise). Do not use them as feature proof.
12. Tooling notes: jq reads only from stdin in this environment; node cannot start from the deep extract path; long bash commands containing single quotes can fail to parse, so files were written with the Write tool.

## Appendix A. IPC method inventory (registered namespaces; from dist/ipc/handlers and dist/ipc/events)

Method names per namespace (cleaned of JavaScript keywords; source files in dist/ipc/handlers/<name>.js):
- addonLocal: resolveLocalPreview, openLocalScope, closeLocalScope, registerLocalFrameScript, unregisterLocalFrameScript, stageLocalArtifact, takePendingDeepLinkAction, isEnabled, isWorkspaceBindingReady, list, sync, activate, cancelAction, describeAction, reload, remove, removeWithReport, getJson, postJson, acquireDeviceService, renewDeviceService, releaseDeviceService, openFrame, connectFrameDeviceService, pickFrameFiles, grantFrameDroppedFiles, closeFrame, frameStorage.
- addonWorkspace: resolveBinding, resolveParentBinding.
- appearance: broadcastConfig.
- auth: setToken, removeToken, notifyLoginSuccess.
- browser: activateUserTab, attachWebviewTab, cancelDownload, capturePagePng, captureScreenshot, clearBrowsingData, clearSavedPasswords, closeTab, continueAfterCertificateError, detachWebviewTab, dismissPasswordSelection, downloadPdfDocument, getDownloads, getNavigationState, getPdfPageState, getPipState, inspectElement, listSavedPasswords, openDownload, openDownloads, openUserLink, openUserTab, registerBrowserWebview, requestExternalApplicationFromAddressBar, resetDraftWorkspace, respondToExternalApplication, returnToSafety, setActiveSession, setBrowserReloadActive, setBrowserShortcutsActive, showDownloadInFolder, showLinkContextMenu, transferSessionWorkspace, unregisterBrowserWebview, updatePdfViewerUi.
- browserDataImport: listProfiles, importProfile.
- capture: getScreenRecordingPermission, requestScreenRecordingPermission, consumePendingSendToManus.
- computerUse: listInstalledApps, getAppIcon.
- connectorCompletion: begin, cancel.
- debugTools: getState, unlock, openDevTools.
- desktopActivityIndicator: getState, rendererReady, setEnabled, setExpanded, updateSnapshot, notifyTask, updateMaterialSurfaces, clearMaterialSurfaces, openMainWindow, consumePendingTaskNavigation, showContextMenu, startDragging, dragTo, stopDragging, setMouseInteractionEnabled, openTask, markTaskRead, dismissTask, stopTask.
- desktopOperator: watchWorkspaceFiles, unwatchWorkspaceFiles, getState, getWindowsSafeModeStatus, enableWindowsSafeModes, setEnabled, resolveSessionWorkspace, getWorktreeRetention, listWorktreeRetention, setWorktreeRetention, previewWorktreeCleanup, prepareSelectedWorktreeCleanup, cleanupWorktrees, listWorkspaceLocations, listWorkspaceDirectory, searchWorkspaceFilesByAbsolutePath, searchWorkspaceFiles.
- env: setOverrideAndRelaunch.
- globalVoiceInput: beginTarget, checkTargetAccess, getMicrophoneConfig, setMicrophone, insertText, clearTarget, consumePendingAction, setPhase, setAudioLevel, showMessage.
- keepAwake: getState, setEnabled.
- localMcp: listStaleAutoWorkspacePaths, ensureAutoWorkspaceFolder.
- presenter: open, ready, getData.
- remoteControl: consumeRequestsChanged.
- screenLive: requestStart, getState, stop.
- shortcut: getConfig, getCapabilities, beginRecording, endRecording, setEnabled, update, reset, setWindowBindings.
- skillRecording: deliverRemote, takeRemoteDeliveries, completeRemoteDelivery, openRemoteTask, getRemoteAccountContext, authorizeRemoteDelivery, getAccountContext, authorizeDelivery, open, watch, status, readVoiceSegment, completeTranscription, readArtifact, cleanup, discard.
- system: addApplicationPath, askMicrophonePermission, askNotificationPermission, checkMicrophonePermission, checkNotificationPermission, checkScreenCapturePermission, consumeFileGrant, copyPageRegionAsImage, createLocalDocument, getGitHubPullRequestPreview, getLocalFileApplications, getLocalMediaPreviewUrl, getLocalPathInfo, getSystemLocale, getWorkspaceGitInfo, getWorkspaceGitInfoResult, notifyNetworkOffline, notifyNetworkOnline, openExternal, openFullDiskAccessSettings, openLocalPath, openLocalPathWithApplication, openMediaAccessSettings, openNotificationSettings, openScreenCaptureSettings, readClipboardFilePaths, readFileAsArrayBuffer, readLocalFileForPreview, readLocalMarkdownResource, renameLocalFile, resolveLocalFileLink, saveLocalFileBackup, saveLocalPathAs, setQuitConfirmationLabels, showFileDialog, showFolderDialog, showLocalPathContextMenu, showLocalPathInFolder, switchWorkspaceGitBranch, unwatchLocalFile, watchLocalFile, writeLocalFile.
- terminal: listRecoverableSessionLocators, listRecoverable, create, transferSessionWorkspace, write, resize, attach, setActive, close.
- theme: broadcastConfig.
- updater: getVersion, getPolicyContext, openStore, getStatus, checkForUpdates, restartNow, showDownloadedInstaller, consumeInstalledUpdateVersion, setAutoInstallWhenTaskIdle, setTaskRunning.
- window: showAppMenu, getZoomFactor, changeZoom, showMainWindow, startTaskDragPreview, stopTaskDragPreview, openTaskInNewWindow, openScreenLiveViewer, getScreenLiveViewerData, fitScreenLiveViewer, finishScreenLiveViewerClose, getTaskWindowLaunchData, getTaskWindowPinned, setTaskWindowPinned, minimize, maximize, close, isMaximized, toggleFullscreen, isFullscreen.
- githubPullRequestPreview service (not registered as a namespace): normalizeUrl, get, resolveExecutable, execute, resolveExecutionError, parse, resolveStatus, hasFailedCheck, hasPendingCheck.

Event names per module (dist/ipc/events/<name>.js and registry allEvents / allScopedEvents):
- addonLocal: onCatalogChanged. appearance: onConfigUpdated. theme: onConfigUpdated.
- auth: onLoginError, onLoginSuccess.
- browser: onDownloadStateChanged, onMediaPermissionNotice, onNativeShortcut, onNavigationStateChanged, onPipStateChanged.
- capture: onSendToManus.
- desktopActivityIndicator: onMarkTaskReadRequested, onStateChanged, onStopTaskRequested.
- desktopOperator: onWorkspaceFilesChanged.
- keepAwake: onStateChanged.
- menu: onCheckForUpdates, onDownloadMobileApp, onFindInPage, onNewProject, onNewScheduledTask, onNewTask, onSettingsRequested.
- myComputer: onLifecycleChanged.
- navigation: onAppDeepLink, onConnectorCompletionStarted, onLocalAddonAction, onNavigateToSession, onOpenBrowserSessionLink, onOpenLocalFiles, onOpenSettingsRoute.
- remoteControl: onRequestsChanged.
- screenLive: onStateChange.
- shortcut: onConfigChanged, onTriggered, onWindowCommand.
- skillRecording: onAccountChanged, onRemoteAccountChanged, onRemoteDeliveryPending, onStatusChanged.
- system: onAppActivated, onLocalFileChanged.
- terminal: onEvent.
- updater: onPolicyTestChanged, onStatusChanged.
- window: onFullscreenChanged, onScreenLiveViewerCloseRequested, onZoomChanged.

## Appendix B. Managed bundled addon archives (install resources/managed-addons; names only, not extracted)

| archive (sha256 prefix) | addon id and version (manifest.json) | entries | bytes | top-level content seen |
|---|---|---|---|---|
| 06446d70... | manus-workflow 0.0.23 | 4 | 728,125 | addon.json, runtime/main.mjs, runtime/evaluator/thread.mjs, skills/workflow-composer/SKILL.md |
| 2736ed4e... | manus-slides 0.0.1-prod.37468226838.d1a18268 | 9 | 170,163 | addon.json, browser/slides-renderer (index.html, measurement.js, worker.js), runtime/mcp/deviceServer.mjs, runtime/skill (SKILL.md, html-syntax.md, pptx-xml-syntax.md) |
| 3fe93029... | manus-triggers 0.0.58 | 10 | 47,045 | addon.json, addon-meta.json, runtime/mcp/server.mjs, runtime/skill (SKILL.md, references/builtin-tools.json, references/connector-runtime.md) |
| 68d3be41... | manus-tasks 0.0.15 | 3 | 8,243 | addon.json, runtime/server.mjs, skills/manus-tasks/SKILL.md |
| 794f4c40... | manus-webdev 0.0.68 | 4,571 | 24,016,397 | addon.json, runtime/checkpoint-preview.html, runtime/diagnostic/manus-webdev-lsp.mjs and LSP presets (astro, css, dockerfile, gopls, graphql, html, json, markdown, ...) |
| 8b934477... | manus-video-editor 0.1.0-prod.37756172562.1 | 3,230 | 41,300,797 | addon.json, addon-meta.json, bin/ffmpeg.exe, dist/ (artifact-root.mjs, browser-runtime, core, diagnostics, editor, headless, build-metadata.json) |
| 9c4f8383... | manus-storage 0.1.5 | 3 | 9,444 | addon.json, runtime/mcp.js, skill/SKILL.md |
| f501c7fd... | manus-trip 0.0.1-prod.37279479499.7edf05fb | 7 | 91,185 | addon.json, runtime/mcp/server.mjs, runtime/package.json, runtime/skill (SKILL.md, display.md, hotel-search.md, hotel-query-examples.md) |

Unsupported addon ids (catalog entries without a bundle for this platform): manus-clue, manus-cronometer, manus-fitbit, manus-myfitnesspal, manus-oura, manus-plaid, manus-polar, manus-trainingpeaks, manus-withings.
