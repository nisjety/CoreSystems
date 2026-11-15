# Codex desktop: in-app browser, browser-use and computer-use inventory (Audit 2, scout)

Read-only inventory for a later deep-dive agent. Nothing was executed, modified or installed.

Provenance tags: REA = REA analysis of a shipped artifact. SRC = read in source, bundle, manifest or installed program file (grep counts count as SRC presence evidence). DOC = packaged documentation (plugin docs, api.json, SKILL.md, policy docs). INF = inference, basis stated.

Base paths used below:
- EXTRACT = C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-AppData-Roaming-Claude-scratch-workspaces-5e23d905-dd91-4518-8ff4-252efcbe0520-b5ff8b3a-437c-4499-9a7f-469de994f319-scratch-2026-10-12-69a4ee/1e4c8548-5aa8-429f-9d17-693211694c40/scratchpad/extract/codex
- INSTALL = C:/Program Files/WindowsApps/OpenAI.Codex_26.1007.2314.0_x64__2p2nqsd0c76g0/app
- PLUG = INSTALL/resources/plugins/openai-bundled/plugins

## 0. Method and limits of this pass

- REA used: analyze_javascript_application once, on EXTRACT/.vite/build (format directory, detail summary, record-and-continue, 2048 MB heap, 900 s timeout). Result: 34 JS files parsed, 0 parse failures, about 2.7M AST nodes, 1,113 findings. trace_application_feature once (channel seed codex_desktop:browser-page-event): no match. inspect_analysis_view was not called separately; the summary view came back with the analysis. Other REA tools (strings, PE resources, binary overview, artifact inspect) were not used and no binary REA session was opened.
- Not read by design: app user-data directories and credential, session, cookie, password, history and download stores. Settings routes for passwords and history exist (see 3.8) but were not inspected.
- Presence of a string or call is not proof of default enablement or runtime reachability.

## 1. Identity

- Product: Codex (OpenAI), Windows desktop app. MSIX package OpenAI.Codex 26.1007.2314.0; app version 26.1007.21434; codexBuildNumber 13901; flavor prod. [SRC: EXTRACT/package.json; INSTALL/resources/owl-app.ini]
- Shell: Electron. package.json declares electron 42.3.0, @electron-forge 7.11.2 with plugin-vite, vite 8.2.0, rolldown babel plugin, vitest 4.1.10, TypeScript 7.0.2, oxlint and oxfmt scripts. Main entry .vite/build/early-bootstrap.js. [SRC]
- Chromium runtime: INSTALL ships chrome.dll (329 MB), chrome_proxy and pwa launchers, and 155.0.8059.40.manifest. INSTALL/resources/owl-electron-app.json names the runtime "owl" with a runtime archive hash. [SRC]
- Relationship between declared Electron 42 and the owl Chromium runtime: INF. package.json has owl and owl-shell dev scripts that run the app inside the owl shell, so the owl runtime is probably a custom Chromium build exposing Electron-compatible APIs (medium confidence).
- Native backend: Rust core in INSTALL/resources/codex.exe (334 MB), plus INSTALL/resources/codex (294 MB, no extension, not analysed), codex-code-mode-host(.exe), codex-command-runner.exe, codex-windows-sandbox-service.exe and codex-windows-sandbox-setup.exe. Browser-relevant strings in codex.exe: browser_use, browser_use_full_cdp_access, computer_use, computer_use_only, browser_and_computer_use, browser_annotation_api, in_app_browser, browser_settings_import, cua_repl, webmcp, enforce_residency, oauth_client_registration. [SRC, grep only]
- Agent JS runtime: INSTALL/resources/cua_node (Node 24.21.0-cua.1; bin/node.exe, bin/node_repl.exe, node_modules; manifest runtime 0.0.29). [SRC]
- Renderer: EXTRACT/webview (index.html, detached-window.html, assets/ with about 19.4k files, service-worker/events.js, apps/ IDE icons for "open in", images/, licenses/). Framework not confirmed; react-dom markers in 3 asset files (INF, low confidence).
- Dependencies by capability (EXTRACT/package.json, SRC):
  - Browser and agent: browser-api, browser-client, browser-backend-common (linked to ../../../lib/oai_browser_use/*, not present in the extract), browser-common, playwright 1.61.1 (devDependency, used by a dev REPL script), capnweb 0.11.1 (RPC), @openai/codex-app-tools-plugin, app-server-manager, app-server-types, protocol, shared-node, commands, external-agent-migration, git-worker.
  - Processes and OS: node-pty, objc-js 1.5.0 (macOS), ws, bufferutil, utf-8-validate, shlex, ssh-config, @napi-rs/canvas 0.1.89 (dev).
  - Data: better-sqlite3 12.11.1, yjs 13.6.31, zod 4.4.3, smol-toml, yaml 2.9.0, tar 7.5.16, extract-zip, mime-types, fs-extra (dev), @parcel/watcher 2.5.6, @tanstack/query-core 5.101.2, lodash.
  - Protocols: vscode-jsonrpc 8.2.0, vscode-languageserver-protocol 3.17.5, vscode-uri 3.1.0.
  - Domains: tldts 7.4.9 (INF: used for origin and domain allowlist logic).
  - Telemetry: @sentry/electron 7.5, @sentry/node and @sentry/core 10.29.0, @opentelemetry/api 1.9.0, core 2.2.0, otlp-transformer 0.208.0, resources 2.2.0, sdk-trace-base 2.2.0.
  - Desktop UX: electron-context-menu 4.1.2.
  - Packaging and signing: @electron-forge makers (msix, zip, deb, rpm), @electron/fuses, @electron/notarize, @electron/windows-sign 1.2.2, electron-windows-msix 2.0.4.
  - Non-browser: @worklouder/device-kit-oai and @worklouder/wl-device-kit (hardware kit, out of scope).
- Browser plugin dependencies (PLUG/browser/node_modules, SRC): classic-level with abstract-level, level-transcoder, level-supports, napi-macros and node-gyp-build (a native LevelDB store). Plugin scripts ship WASM: browser-accessibility.wasm.br and zxing_reader.wasm. [SRC]
- Source-of-truth note from manifests: the browser plugin homepage and repository point to an openai monorepo path lib/oai_browser_use/plugin. That source is not in this extract. [SRC]

## 2. Directory and module map

- EXTRACT/ (Electron app root, 594 MB): package.json; .vite/build/ (34 JS bundles, about 10.3 MB of text); webview/ (renderer); node_modules/ (not enumerated); native-menu-locales/ (64 locale JSON files, no English file in the listing).
  - .vite/build/main-BklS_2Y2.js (3.8 MB): main process. About 3.7k browser references. Browser host, session, tab, CDP and IPC logic. [SRC]
  - .vite/build/browser-page-preload.js (527 KB): preload injected into browsed pages. About 300 annotation references. [SRC]
  - .vite/build/worker.js (2.4 MB): worker with computer-use and browser references; role INF.
  - .vite/build/src-BJv5Mk7n.js (1.6 MB) and src-CP7XXlpZ.js (204 KB): secondary modules, heavy on permission, download and cookie terms; role not confirmed.
  - .vite/build/bootstrap-487kJHw_.js (461 KB): bootstrap module with download (109) and debugger (19) references.
  - .vite/build/browser-background-switches-Zzmx0u5p.js (144 KB): Chromium switches for background browsing.
  - .vite/build/preload.js, sandbox-preload.js, early-bootstrap.js, mcp-app-attachment-*.js, conversation-turn-mutations-*.js, skills-*.js, service-*.js, desktop-open-path-queue-*.js, windows-file-copy-*.js, upload-*.js, capture-*.js: smaller helpers, 0 to 23 browser references each.
  - webview/index.html (15 KB) and webview/detached-window.html (3 KB): renderer entry pages.
- INSTALL/ (Windows install, read-only): Codex.exe (launcher), ChatGPT.exe, chrome.dll and Chromium resource files, owl-shell-runtime.json, resources/ (which holds owl-app.ini, owl-electron-app.json and masque-proxy-origins.txt; the last has one origin, a codex-portals endpoint).
  - INSTALL/resources/app.asar (567 MB) and app.asar.unpacked/node_modules (not separately extracted; EXTRACT is the copy used).
  - INSTALL/resources/plugins/openai-bundled/plugins/: browser, chrome, computer-use, unified-computer-use, codex-app-tools, code-review, deep-research, latex, visualize.
  - INSTALL/resources/plugin-signatures/openai-bundled/receipt.json: signed plugin receipt list.
  - INSTALL/resources/cua_node/: Node runtime for node_repl and cua. INSTALL/resources/native/: computer-use-app-icons.node, hid-topology-watcher.node, remote-control-device-key.node, windows-account.node, windows-updater.node.
  - INSTALL/resources/accessibility/reading_mode_gdocs_helper_manifest.json (appears to be a Chrome extension manifest for a Google Docs reading helper).
  - INSTALL/resources/skills/, codex.exe, codex-code-mode-host.exe, rg.exe, tectonic/ (LaTeX), default_app/, artifact-template-picker/ (out of scope).
  - PLUG/browser/: .codex-plugin/plugin.json; skills/control-in-app-browser/ (SKILL.md, agents/openai.yaml); docs/ (about 22 md files plus api.json, 66 KB, and documents.json, 3.9 KB); docs/capabilities/browser/ (management, viewport, visibility) and docs/capabilities/tab/ (cdp, webmcp, botDetection, browserAuth, pageAssets); scripts/ (browser-service.mjs 1.45 MB, browser-client.mjs 170 KB, installed-browsers.js, check-extension-installed.js, check-native-host-manifest.js, chromium-browser-diagnostics.mjs, chrome-is-running.js, open-chrome-window.js, extension-ids.json, zxing_reader.wasm, browser-accessibility.wasm.br); assets/ (brave, chrome, edge, opera, vivaldi icons). [SRC]
  - PLUG/chrome/: same layout, plus extension-host/windows/x64/extension-host.exe (native messaging host), skills/control-chrome/SKILL.md, scripts/installManifest.mjs. [SRC]
  - PLUG/computer-use/: docs/ (api.md, guidance.md, confirmations.md), skills/computer-use/SKILL.md, assets/app-icon.png. [SRC]
  - PLUG/unified-computer-use/: .codex-plugin/plugin.json and .mcp.json only (cua_repl MCP server, disabled by default). [SRC]
  - PLUG/codex-app-tools/: .mcp.json, server.mjs, scripts/launch_codex_app_tools_mcp(.cmd). [SRC]

## 3. Feature candidates by taxonomy category

Each bullet: feature, detail, pointer, provenance.

### 3.1 Acquisition API surface

- **Batch content fetch**: browser.tabs.content({urls, contentType, timeoutMs}) loads each URL in a temporary tab and returns html, text or domSnapshot. Pointer: PLUG/browser/docs/api.json (TabsContentOptions, TabsContentType). [DOC]
- **Tab navigation primitives**: Tabs.get/list/new/selected; Tab.goto/back/forward/reload/close/title/url/id. Pointer: api.json (interfaces Tabs, Tab). [DOC]
- **Browser discovery**: Browsers.list/get/getDefault/getForUrl; Agent.browsers. Pointer: api.json (Browsers, Agent). [DOC]
- **Browser history read**: Browser.history with BrowserHistoryEntry and BrowserHistoryOptions. Pointer: api.json. [DOC]
- **Packaged docs lookup**: Agent.documentation and Documentation.get read packaged browser-use docs by name. Pointer: api.json; docs/documents.json. [DOC]
- **Google Docs export**: ContentAPI.exportGsuite. Pointer: api.json (ContentAPI). [DOC]
- **YouTube transcript export**: ContentAPI.exportYouTubeTranscript. Pointer: api.json (ContentAPI). [DOC]
- **Generic content export**: ContentAPI.export. Pointer: api.json (ContentAPI). [DOC]
- **Page asset inventory and bundling**: pageAssets.list (script, other, font, image, stylesheet, video, inline SVGs) and bundle (kind or asset-id filters, manifest path, failure list). Pointers: PLUG/browser/docs/capabilities/tab/pageAssets.md; browser-service.mjs method capabilities/tab/pageAssets. [DOC, SRC]
- **Screenshots**: Tab.screenshot and PlaywrightAPI.elementScreenshot with clip and fullPage options; JPEG output. Pointers: api.json (ScreenshotOptions); docs/screenshots.md. [DOC]
- **Page screenshot in the main process**: Page.captureScreenshot (CDP) and webContents.capturePage (Electron, 8 call sites). Pointer: .vite/build/main-BklS_2Y2.js. [SRC]
- **CDP network observation**: Network.requestWillBeSent, responseReceived, loadingFinished, loadingFailed, getResponseBody, getRequestPostData. Pointer: main-BklS_2Y2.js. [SRC]
- **Downloads**: PlaywrightDownload.path; media download on the CUA, DOM CUA and locator APIs (downloadMedia); Page.downloadWillBegin; download gating (consumeAllowedBrowserUseDownload). Pointers: api.json; main-BklS_2Y2.js. [DOC, SRC]
- **Accessibility tree**: AXAPI.get plus a WASM accessibility engine (browser-accessibility.wasm.br). Pointers: api.json (AXAPI); PLUG/browser/scripts. [DOC, SRC]
- **Barcode and QR reading**: zxing_reader.wasm ships with the browser plugin; callers not located (INF: reads codes from page or screenshot images). Pointer: PLUG/browser/scripts/zxing_reader.wasm. [SRC, INF]
- **Reading-mode helper for Google Docs**: manifest-like file in INSTALL/resources/accessibility; no reference in .vite/build (grep count 0). Pointer: INSTALL/resources/accessibility/reading_mode_gdocs_helper_manifest.json. [SRC, low confidence on use]

### 3.2 Output formats and content processing

- **Content representations**: html, text and domSnapshot content types; DOM snapshot via PlaywrightAPI.domSnapshot; visible DOM via DomCUAAPI.get_visible_dom; ElementInfo with rectangles and selectors. Pointer: api.json. [DOC]
- **Screenshot-in-answer convention**: screenshots are JPEG and should be embedded inline with Markdown image syntax when the user asked for them. Pointer: PLUG/browser/docs/screenshots.md. [DOC]
- **Clipboard read and write**: TabClipboardAPI read, readText, write, writeText with clipboard item and entry types. Pointer: api.json. [DOC]
- **File chooser and upload**: PlaywrightFileChooser.setFiles and isMultiple, with the chooser event awaited before the click. Pointers: PLUG/browser/docs/file-uploads.md; api.json. [DOC]
- **Content-type handling**: service handles text/html, text/plain, text/css, application/pdf, application/json and image/jpeg. Pointer: PLUG/browser/scripts/browser-service.mjs. [SRC]
- **Change tracking and diffing**: none located. [SRC, not found]
- **Schema or prompt-driven structured extraction**: none located in browser code; structured results are the typed API outputs only. [SRC, not found]
- **Branding and metadata extraction**: none located. [SRC, not found]

### 3.3 Rendering and browser engines

- **In-app browser (IAB) on Electron webContents**: tabs are web contents in partitioned sessions named persist:codex-browser-app-route:<route>:host:<host>. Pointer: .vite/build/main-BklS_2Y2.js (route parsing, constant $e). [SRC]
- **Electron APIs in use**: webContents.debugger attach, sendCommand, on, detach, isAttached; setWindowOpenHandler; navigationHistory; capturePage; executeJavaScript; insertCSS; findInPage; downloadURL; setZoomFactor; openDevTools; webRequest listeners; protocol.handle; registerSchemesAsPrivileged; cookies get, set, remove, flushStore; setBackgroundThrottling; sendInputEvent; setAudioMuted; loadExtension. Pointer: main-BklS_2Y2.js. [SRC]
- **CDP domains used**: Runtime (evaluate, callFunctionOn, addBinding, getProperties); Page (navigate, reload, getFrameTree, createIsolatedWorld, addScriptToEvaluateOnNewDocument, navigationBlocked, downloadWillBegin); Target (attach, getTargets, setAutoAttach, closeTarget); DOM (getDocument, querySelector, describeNode); Input (dispatchMouseEvent, dispatchKeyEvent, insertText); Network; Emulation (setDeviceMetricsOverride, setEmulatedMedia, setFocusEmulationEnabled); Debugger (enable, pause, setPauseOnExceptions); Fetch (only disable is present). Pointer: main-BklS_2Y2.js. [SRC]
- **Viewport override**: browser capability viewport set and reset; device toolbar toggle for new tabs (setting browser-show-device-toolbar-in-new-tabs). Pointers: PLUG/browser/docs/capabilities/browser/viewport.md; main-BklS_2Y2.js. [DOC, SRC]
- **Visibility control**: browser visible or hidden, background by default; browser_visibility_set log. Pointers: PLUG/browser/docs/capabilities/browser/visibility.md; main-BklS_2Y2.js. [DOC, SRC]
- **Background networking off**: Chromium switches disable-background-networking, disable-component-update and codex-browser-background-networking-disabled. Pointer: .vite/build/browser-background-switches-Zzmx0u5p.js. [SRC]
- **Injected page preload with annotation runtime**: preload runs in browsed pages and posts events to the host over codex_desktop:browser-page-event. Pointer: .vite/build/browser-page-preload.js. [SRC]
- **Cloud Browser**: server-hosted browser tabs. Named in the tab docs and in the manual-handoff declaration; cloudBrowserUse gate in the model settings schema. Engine and hosting not identified. Pointers: api.json (Tab.requestManualHandoff); PLUG/browser/docs/capabilities/tab/botDetection.md; main-BklS_2Y2.js. [DOC, SRC]
- **Local browser use**: gated as localBrowserUse in the same settings schema. Pointer: main-BklS_2Y2.js. [SRC]
- **External Chromium control (Brave, Chrome, Edge, Opera, Vivaldi)**: drives the user's installed Chromium browsers through a ChatGPT extension and a native messaging host. Pointers: PLUG/chrome/extension-host/windows/x64/extension-host.exe; PLUG/browser/scripts/extension-ids.json; PLUG/browser/docs/chrome-troubleshooting.md. [SRC, DOC]
- **MCP Apps side-panel control**: the mcpapps backend drives MCP Apps opened in side-panel tabs; cannot create or navigate tabs. Pointer: PLUG/browser/docs/mcp-apps.md. [DOC]
- **MCP app sandbox**: session restriction on requests; localhost http and ws allowed only when allowLocalDevelopment is set; blocked requests are logged under mcp_app_sandbox.* events. Pointer: main-BklS_2Y2.js. [SRC]
- **Restricted navigation**: allowed schemes about, blob, data, http, https; isRestrictedBrowserNavigation; navigation-blocked listeners. Pointer: main-BklS_2Y2.js. [SRC]
- **Error page**: loadBrowserErrorPage. Pointer: main-BklS_2Y2.js. [SRC]
- **Find in page**: findInPage and stopFindInPage. Pointer: main-BklS_2Y2.js. [SRC]
- **Zoom**: setZoomFactor and setZoomLevel; rightPanelBrowserCanZoom and bottomPanelBrowserCanZoom flags. Pointer: main-BklS_2Y2.js. [SRC]
- **Print to PDF**: none found (printToPDF count 0). [SRC, not found]
- **Reader mode**: none found in main bundle. [SRC, not found]

### 3.4 Anti-bot, proxies and reliability

- **Bot-detection reporting**: the botDetection capability reports captcha_failed, access_denied, challenge_loop and unexpected_bot_error as internal telemetry. The docs exclude bare HTTP 403 and network-policy denials (ERR_BLOCKED_BY_ADMINISTRATOR, proxy, DNS, TLS, timeouts). Pointer: PLUG/browser/docs/capabilities/tab/botDetection.md. [DOC]
- **CAPTCHA and interstitial policy**: CAPTCHA solving needs action-time confirmation; bypassing safety interstitials or paywalls requires hand-off to the user. Pointers: PLUG/browser/docs/confirmations.md; PLUG/browser/docs/browser-safety.md. [DOC]
- **Cloud tunnel to a portal origin**: QUIC tunnel with a proxy origin file (masque-proxy-origins.txt), target validation and assertDestinationAllowed checks; logger cloud-portals. Pointers: INSTALL/resources/masque-proxy-origins.txt; main-BklS_2Y2.js. [SRC]
- **Generic retry, backoff and rate-limit code**: present in the main bundle (retry 263, backoff 13, rate-limit 17 hits), mostly generic and not verified for browser flows. Pointer: main-BklS_2Y2.js. [SRC, INF]
- **robots.txt handling**: none in the main bundle (count 0); the plugin service was not checked for it. [SRC, not found]
- **Stealth or fingerprint evasion**: none found (stealth 0; setUserAgent, setGeolocationOverride, setTimezoneOverride, setLocaleOverride all 0). The word fingerprint appears 49 times, context not verified. [SRC, not found]
- **Proxy configuration**: no Electron session proxy setter found (setProxy and proxyRules 0). [SRC, not found]
- **Caching and freshness**: none located. [SRC, not found]
- **Enterprise and residency policy**: enforce_residency and experimental_network strings in codex.exe; administrator-block error referenced in docs. [SRC, DOC, INF on purpose]

### 3.5 Orchestration and scale

- **Per-conversation browser tabs**: storage key thread-browser-tabs-v1; threadBrowserTabCount and threadBrowserUseTabCount; admitBrowserTab; presentedBrowserTabIdsByOwner. Pointer: main-BklS_2Y2.js. [SRC]
- **Turn-scoped tab lifecycle**: agent-created tabs close when the turn ends; user-opened tabs stay; markDeliverable and markHandoff are turn-scoped. Pointer: PLUG/browser/docs/tab-cleanup-iab.md. [DOC]
- **Turn-end hooks**: Interrupt, SubagentStop and Stop hooks call node_repl turn_ended for browser, chrome and computer-use; cua_repl for unified-computer-use. Pointers: PLUG/*/.codex-plugin/plugin.json. [SRC]
- **CDP event cursor and buffer**: readEvents with cursor, afterSequence, limit 1 to 1000, hasMore and truncated flags. Pointer: PLUG/browser/docs/capabilities/tab/cdp.md. [DOC]
- **Limits and timeouts**: webMcpMaxTools default 100; cua_repl output_token_limit 25000; startup_timeout_sec 120; comment-screenshot timeout; cursorArrivalTimeoutMs. Pointers: main-BklS_2Y2.js; PLUG/unified-computer-use/.mcp.json. [SRC]
- **Pending navigation tracking**: trackPendingBrowserUseNavigation, clearPendingBrowserUseNavigation, openingPagePromisesByBrowserTabId. Pointer: main-BklS_2Y2.js. [SRC]
- **Crawl scoping, sitemaps, scheduling, distributed workers**: none located; crawling is driven by explicit URL lists. [SRC, not found]

### 3.6 Platform and ecosystem

- **Bundled browser plugin**: aliases @browser and @browser-use; the in-app browser is the default target for localhost and file targets. Pointer: PLUG/browser/.codex-plugin/plugin.json. [SRC]
- **Chrome plugin**: controls the user's Chrome state (tabs, sign-ins, cookies, extensions) and requires the ChatGPT Chrome extension. Pointer: PLUG/chrome/.codex-plugin/plugin.json. [SRC]
- **Computer-use plugin**: desktop app control on Windows. Pointer: PLUG/computer-use/.codex-plugin/plugin.json. [SRC]
- **Unified computer-use plugin**: app-managed browser automation runtime through the cua_repl MCP server, disabled by default. Pointer: PLUG/unified-computer-use/.mcp.json. [SRC]
- **Signed plugin receipt**: RSA PKCS1 SHA-256 receipt listing each plugin archive with size and hash; production environment. Pointer: INSTALL/resources/plugin-signatures/openai-bundled/receipt.json. [SRC]
- **Skills**: control-in-app-browser (browser), control-chrome (chrome), computer-use (computer-use). Pointers: each plugin's skills/*/SKILL.md. [SRC, DOC]
- **Surface-selection rule**: explicit browser mentions must use the browser tools; OS open commands or shell are not a substitute unless the user approves. Pointer: PLUG/browser/.codex-plugin/plugin.json (description); skills/control-in-app-browser/SKILL.md. [DOC]
- **Mention deep links**: plugin://browser@openai-bundled?mention=tab-v1 (IAB tab), plugin://browser@openai-curated-remote, plugin://chrome@openai-bundled, plugin://chrome-internal and plugin://chrome-dev variants. Pointers: PLUG/browser/docs/tab-claiming-chrome.md; tab-mentions-iab.md. [DOC]
- **Composer tab mentions**: browserTabMentions, addBrowserTabs, includeBrowserTabsLabel, emptyBrowserTab, browserTabMenu. Pointer: EXTRACT/webview/assets (renderer). [SRC]
- **Native menu localization**: native-menu-locales (64 locale JSON files). Pointer: EXTRACT/native-menu-locales/. [SRC]
- **Chrome extension listing**: store listing and extension ids for five browser families. Pointer: PLUG/browser/scripts/extension-ids.json. [SRC]

### 3.7 Observability and operations

- **Browser backend logs**: browser_use_iab_backend_startup_started, _ready and _failed; browser_use_setup_failed; browser_session_lookup_disabled; browser_visibility_set; browser_viewport_set and _reset; browser-restore-history-failed. Pointer: main-BklS_2Y2.js. [SRC]
- **Browser telemetry**: recordBrowserNavigation, recordBrowserAddressCommit, recordBrowserAutocompleteSuggestionsShown, recordBrowserAnnotationFeature, recordAnnotationFeatureUse, browser-sidebar-usage. Pointer: main-BklS_2Y2.js. [SRC]
- **Error and crash telemetry**: Sentry (electron, node, core), OpenTelemetry (api, sdk-trace-base, otlp-transformer), and a trigger-sentry-test channel. Pointers: EXTRACT/package.json; main-BklS_2Y2.js. [SRC]
- **Browser diagnostics scripts**: chromium-browser-diagnostics.mjs, check-extension-installed.js, check-native-host-manifest.js, chrome-is-running.js, installed-browsers.js. Pointer: PLUG/browser/scripts/. [SRC]
- **Troubleshooting docs**: bootstrap, browser, chrome and Chrome file-upload troubleshooting. Pointers: PLUG/browser/docs/*troubleshooting.md. [DOC]
- **Developer console logs**: TabDevAPI.logs with TabDevLogEntry and log options. Pointer: api.json. [DOC]
- **Heap snapshot analysis**: memory:analyze-snapshot script (dev). Pointer: EXTRACT/package.json. [SRC]
- **Replay of observed actions**: the CDP readEvents cursor pattern lets an agent observe actions between cursors. Pointer: PLUG/browser/docs/capabilities/tab/cdp.md. [DOC]
- **Sandboxing**: Windows sandbox service and setup executables; computer_use_only sandboxed exec strings in codex.exe. Pointer: INSTALL/resources/codex-windows-sandbox-*.exe. [SRC, INF]

### 3.8 In-app browser shell (user-facing)

- **Tabs and tab commands**: openBrowserTab, close-browser-tab, closeOtherTabs, focusTab, nextTab, previousTab, reopenClosedTab, showWorkspaceTabView, toggleWorkspaceTabs. Pointer: main-BklS_2Y2.js (codex.command.* ids). [SRC]
- **Address bar**: focusBrowserAddressBar; autocomplete telemetry; URL normalization; full-URL display setting (browser-show-full-url). Pointer: main-BklS_2Y2.js. [SRC]
- **Navigation controls**: back, forward, reload, hard reload (Force Reload Browser Page); mouse back and forward buttons (browser-sidebar-runtime-mouse-navigation). Pointers: main-BklS_2Y2.js; browser-page-preload.js. [SRC]
- **Panels and placement**: toggle-browser-panel, right and bottom panel browser tab bindings, toggleMaximizeSidePanel, toggleBottomPanel, browser-sidebar-manager, detached window page. Pointers: main-BklS_2Y2.js; EXTRACT/webview/detached-window.html. [SRC]
- **Default browser and external links**: isDefaultBrowser, defaultBrowserName and defaultBrowserPath, useExternalBrowser, openBrowserExternalUrl, external-browser-link-shortcut; bundle identifiers for Brave, Arc and Chromium. Pointer: main-BklS_2Y2.js. [SRC]
- **Keyboard shortcut**: browser-scoped default keybinding CmdOrCtrl+Shift+O (command mapping not confirmed). Pointer: main-BklS_2Y2.js. [SRC]
- **Downloads**: download directory and prompt settings (browser-download-directory, browser-download-prompt-enabled); download history (electron-browser-download-history); will-download handler (7 hits); download_approval_mode (default always_ask). Pointers: main-BklS_2Y2.js; settings route /settings/browser-use/downloads. [SRC]
- **History**: history approval (iab_history_approval_mode, default always_ask); history settings route /settings/browser-use/history; navigation-history restore on start. Pointers: main-BklS_2Y2.js. [SRC]
- **Passwords**: settings route /settings/browser-use/passwords exists; saving passwords is always-confirm in the confirmation policy. Not inspected. Pointers: main-BklS_2Y2.js; PLUG/browser/docs/confirmations.md. [SRC, DOC]
- **Browser data import**: import options for cookies (schema default true), passwords (default true), history (default false) and extensions (default false); cookieDomainAllowlist; allowUnverifiedExtensions; allowElevatedChromeDecryption. At least one data type must be selected. Pointers: main-BklS_2Y2.js (import schema); codex.exe string browser_settings_import. [SRC]
- **Extensions**: loadExtension call; extensions toolbar setting (browser-extensions-toolbar-visible); electron-browser-extension-recently-used; extension installs need confirmation. Pointers: main-BklS_2Y2.js; PLUG/browser/docs/confirmations.md. [SRC, DOC]
- **Bookmarks**: Chrome-compatible bookmark methods through the management capability (write-restricted). Pointer: PLUG/browser/docs/capabilities/browser/management.md. [DOC]
- **Device emulation toolbar**: toggle for new tabs; setEmulatedMedia once. Pointer: main-BklS_2Y2.js. [SRC]
- **DevTools**: openDevTools; DevToolsWebMCPSupport flag; dev-only devtools:reset script that clears cached extension directories. Pointers: main-BklS_2Y2.js; EXTRACT/package.json. [SRC]
- **Context menu**: electron-context-menu integration. Pointer: EXTRACT/package.json. [SRC]
- **Dialog handling**: alert, confirm, prompt and beforeunload dialogs (AlertDialog, ConfirmDialog, PromptDialog, BeforeUnloadDialog) and getJsDialog. Pointer: api.json. [DOC]
- **Print**: not found. [SRC, not found]

### 3.9 AI-browser control

- **Browser-use JS API**: Agent, Browsers, Browser, BrowserUser, Tabs, Tab, AXAPI, CUAAPI, DomCUAAPI, PlaywrightAPI, PlaywrightLocator, PlaywrightFrameLocator, dialog interfaces, TabClipboardAPI, TabDevAPI, ContentAPI, Documentation (67 declared types). Pointers: PLUG/browser/docs/api.json; PLUG/browser/scripts/browser-client.mjs. [DOC, SRC]
- **Accessibility-based actions**: AX click, drag, get, paste, performSecondaryAction, pressKey, scroll, selectText, setValue, typeText, write. Pointer: api.json (AXAPI). [DOC]
- **Coordinate CUA actions**: click, double_click, downloadMedia, drag, keypress, move, scroll, type. Pointer: api.json (CUAAPI). [DOC]
- **DOM CUA actions**: the same set plus get_visible_dom. Pointer: api.json (DomCUAAPI). [DOC]
- **Playwright-compatible locators**: getByRole, getByText, getByLabel, getByPlaceholder, getByTestId, locator, frameLocator, fill, click, press, pressSequentially, selectOption, check, uncheck, setChecked, waitFor, evaluate, evaluateAll, allTextContents, innerText. Pointer: api.json (PlaywrightLocator). [DOC]
- **Playwright-style page waits**: waitForEvent, waitForLoadState, waitForTimeout, waitForURL, expectNavigation. Pointer: api.json (PlaywrightAPI). [DOC]
- **Raw CDP (development only)**: cdp capability with readEvents and sends to the tab or child targets; gated by browser_use_full_cdp_access (Rust core strings) and unsupportedByDefaultIn cdp. Pointers: PLUG/browser/docs/capabilities/tab/cdp.md; codex.exe strings. [DOC, SRC]
- **WebMCP tools**: fetch page-defined tools per tab, call them, read descriptions; handles expire per turn; method and event names webmcp_list_tools, webmcp_invoke_tool, webmcp_changed; inspector data. Pointers: PLUG/browser/docs/webmcp.md; PLUG/browser/docs/capabilities/tab/webmcp.md; main-BklS_2Y2.js. [DOC, SRC]
- **Site annotation API**: pages can declare annotation features (siteAnnotationFeatures, canUseSiteAnnotationApi, annotationControls); policy channel codex_desktop:get-browser-annotation-api-policy; per-host permission and disable list. Pointers: main-BklS_2Y2.js; browser-page-preload.js. [SRC]
- **User annotation tools**: comment at point, comment from selection, multi-select, design editor with scrub, image drag into composer, comment screenshots, markers visible, suspension, annotation lease, annotation requests with prompts, send annotations. Pointers: browser-page-preload.js; main-BklS_2Y2.js; EXTRACT/webview/assets. [SRC]
- **Browser sidebar comment overlay**: overlay and runtime sync messages (browser-sidebar-runtime-*). Pointers: main-BklS_2Y2.js; EXTRACT/webview/assets. [SRC]
- **Visual design editing**: browserSidebarDesignEditor, design-scrub events and localBrowserDesignChange. Pointers: EXTRACT/webview/assets; main-BklS_2Y2.js. [SRC, INF on depth]
- **Agent cursor and overlays**: setBrowserUseCursor, sendCursorPointToAvatarOverlay, computer-use cursor location and arrival waits, computerUseOverlay, cuaPIP. Pointer: main-BklS_2Y2.js. [SRC]
- **Takeover signal**: constant openai/user_took_over in browser-service.mjs; isAgentControllingBrowser state; setBrowserUseActive. Pointers: browser-service.mjs; main-BklS_2Y2.js; PLUG/browser/docs/browser-control-interruption.md. [SRC, DOC]
- **Manual handoff**: requestManualHandoff gives the user control of a Cloud Browser tab and marks it for handoff. Pointer: api.json (Tab). [DOC]
- **Tab marks**: markDeliverable keeps a tab as output; markHandoff keeps it for a later turn. Pointers: api.json (Tab); PLUG/browser/docs/tab-cleanup-iab.md. [DOC]
- **Browser visibility and viewport control**: capability get and set visible; viewport set and reset (reset required after a temporary override). Pointers: PLUG/browser/docs/capabilities/browser/*.md. [DOC]
- **Secure sign-in handoff (browserAuth)**: browserAuth.request collects credentials in a ChatGPT-hosted secure form; browser-client fills and submits them without returning values; account creation is a hand-off. Pointers: PLUG/browser/docs/capabilities/tab/browserAuth.md; browser-service.mjs (codex/browserCredentialRecovery). [DOC, SRC]
- **Confirmation policy**: four levels (hand-off required, always confirm, pre-approval works, no confirmation); scope limited to browser actions. Pointers: PLUG/browser/docs/confirmations.md; PLUG/computer-use/docs/confirmations.md. [DOC]
- **Approval modes**: approval_mode, history_approval_mode, iab_history_approval_mode, download_approval_mode and upload_approval_mode (default always_ask); disable_auto_review; assertBrowserUseGlobalPersistentApprovalAllowed. Pointer: main-BklS_2Y2.js. [SRC]
- **Prompt-injection rules**: web content is untrusted; page or tool instructions cannot grant authorization; reading information is distinguished from transmitting it. Pointer: PLUG/browser/docs/browser-safety.md. [DOC]
- **Domain and origin allowlists**: WebMCP allowed and denied origins with a resolver returning site-denied, allowed or default-denied; cookie domain allowlist for import. Pointer: main-BklS_2Y2.js. [SRC]
- **Restricted-navigation enforcement**: assertBrowserUseNavigationAllowed; activation of a navigation restriction; blocked-navigation listeners. Pointer: main-BklS_2Y2.js. [SRC]
- **Node REPL execution sandbox**: node_repl runs model JavaScript under NODE_REPL_* controls (external module, host services pipe, untrusted-environment allowlist, trusted code paths, model-check enforcement, audio flag, tool overrides, trace metadata). Pointers: main-BklS_2Y2.js; INSTALL/resources/cua_node/bin/node_repl.exe. [SRC]
- **Surface-selection guidance**: explicit browser requests, external browser family requests and target-URL requests have separate selection rules. Pointer: PLUG/browser/skills/control-in-app-browser/SKILL.md. [DOC]
- **Desktop computer-use API (sky, window2)**: list_apps, launch_app, list_windows, get_window, get_window_state, activate_window, click, drag, press_key, scroll, set_value, type_text, perform_secondary_action. Pointer: PLUG/computer-use/docs/api.md. [DOC]
- **Desktop automation safety**: Windows automation safety rules, interrupted-turn and recovery guidance. Pointer: PLUG/computer-use/docs/guidance.md. [DOC]
- **Computer-use native pipe**: native pipe flag, service PID tracking, pipe path, ensure and invalidate managed service; e2e script. Pointers: main-BklS_2Y2.js; EXTRACT/package.json (e2e:computer-use-native-pipe). [SRC]
- **Computer-use capture worker**: capture requests and updates, native bridge, cursor location events. Pointers: worker.js; main-BklS_2Y2.js (role INF). [SRC, INF]
- **Computer history**: computer_history_enabled and computer_history_disabled flags; plugin reconcile. Pointer: main-BklS_2Y2.js. [SRC]
- **App icons addon**: computer-use-app-icons.node. Pointer: INSTALL/resources/native/. [SRC]
- **Screen annotation tool**: get_screen_annotations tool name in the renderer. Pointer: EXTRACT/webview/assets. [SRC]
- **Computer-use auto-install**: computerUseAutoInstall and bundled-plugin auto-install disabled flags. Pointer: main-BklS_2Y2.js. [SRC]

### 3.10 Integration-plane overlap

- **Browser data import from installed browsers**: see 3.8; sits on the integration and privacy boundary. Pointer: main-BklS_2Y2.js. [SRC]
- **Google Docs export and YouTube transcript export**: see 3.1. Pointer: api.json (ContentAPI). [DOC]
- **Cloud portals (SSH and QUIC)**: cloud SSH connections and portal credential renewal messages; adjacent to Cloud Browser, relation unverified. Pointer: main-BklS_2Y2.js. [SRC, INF]
- **Remote-hosted picture-in-picture**: codex_desktop:remote-hosted-pip-video-frame channel and remote-hosted PIP layout state. Pointer: main-BklS_2Y2.js. [SRC, INF on purpose]
- **MCP servers**: node_repl (browser and chrome hooks), cua_repl (unified-computer-use), codex-app-tools. Pointers: PLUG/*/.mcp.json. [SRC]
- **Connector OAuth**: oauth_client_registration string in codex.exe. Pointer: INSTALL/resources/codex.exe. [SRC]
- **External agent migration**: external-agent-migration package; external_migration strings in codex.exe. Pointers: EXTRACT/package.json; codex.exe. [SRC]
- **Data sync**: yjs dependency present; browser use of it unverified. Pointer: EXTRACT/package.json. [SRC, INF]

## 4. Extension points

- Plugin manifests: .codex-plugin/plugin.json with hooks, skills and mcpServers fields. Bundled plugin receipts are signed.
- Skills: SKILL.md for browser, chrome and computer-use; agents/openai.yaml for the browser skill.
- MCP servers: node_repl (browser and chrome hooks); cua_repl (unified-computer-use, disabled by default, tools js, js_reset, turn_ended); codex-app-tools (server.mjs).
- Browser-use JS API (api.json) used inside the node_repl REPL; Documentation.get for packaged docs.
- WebMCP: page-defined tools discovered and invoked by the agent (web-side extension point).
- Site annotation API: page-defined annotation features (web-side extension point).
- Mention deep links plugin://... with the tab-v1 format.
- Chrome extension and native messaging host (extension-host.exe, installManifest.mjs, extension ids for five browser families).
- Rust config keys in codex.exe: browser_use, computer_use, browser_annotation_api, in_app_browser, browser_and_computer_use, webmcp, cua_repl, browser_settings_import.
- Dev-only scripts (EXTRACT/package.json): playwright:agent:repl (Playwright over CDP REPL), e2e:computer-use-native-pipe, devtools:reset, owl and owl-shell run scripts.
- CLI: no browser-specific CLI command located (INF).
- Feature gates: browser import, WebMCP, site annotation, cloudBrowserUse, localBrowserUse, browser_use_full_cdp_access.

## 5. Data and protocol hints

- Local storage: better-sqlite3 (app database); classic-level LevelDB (browser plugin); Electron partition storage per browser route; thread-browser-tabs-v1 key prefix. Use of localStorage or IndexedDB not verified. [SRC, INF]
- IPC channels: about 25 literal codex_desktop:* names found by grep, including browser-page-event, browser-sidebar-runtime-message, get-browser-webmcp-policy and get-browser-annotation-api-policy. Most channel expressions are dynamic (REA: 36 of 38 IPC operations unresolved). [SRC, REA]
- Method and event names in the plugin service: capabilities/tab/* and capabilities/browser/*, browser/access, openai/user_took_over, openai/confirmation_policies, openai/client_unsupported, openai/client_is_mobile, codex/toolSurface, codex/browserUse, codex/browserSiteGuidance, codex/browserCredentialRecovery. [SRC]
- CDP over the Electron debugger API (webContents.debugger), standard domains as in 3.3. [SRC]
- RPC libraries: capnweb 0.11.1 and vscode-jsonrpc (LSP style). [SRC]
- Named pipes and paths: NODE_REPL_HOST_SERVICES_PIPE_PATH; computerUseNativePipePath and nativePipeDirectory; serviceNativePipePath. [SRC]
- QUIC/MASQUE tunnel with authorization token, grant and proxy origin; destination check before connection. [SRC]
- Auth: browserAuth secure form; cloud portal credential renewal; ChatGPT account sign-in for Cloud Browser (INF). [SRC, DOC]
- Plugin signing: receipts use RSA PKCS1 SHA-256 with a production environment. [SRC]
- Telemetry: Sentry and OpenTelemetry OTLP transformer present. [SRC]
- Data residency: enforce_residency and experimental_network strings in codex.exe. [SRC]
- Native messaging: Chrome native-host manifest check and install scripts. [SRC]

## 6. Blockers and limits

- REA: main-process semantic analysis was never started (main-BklS_2Y2.js); browser-page-preload.js exhausted the worker heap; many other bundles exceeded the result budget. The graph is partial with 5,025 semantic unknowns. IPC channel names are mostly dynamic, and the trace for codex_desktop:browser-page-event returned no match. That is not evidence of absence.
- No REA analysis of the renderer (about 19.4k asset files), codex.exe or chrome.dll. The one-heavy-analysis-per-app rule was respected and binary sessions were avoided because of the shared active target.
- The browser implementation source (browser-api, browser-client, browser-backend-common and lib/oai_browser_use) is referenced by package.json but absent from the extract. Only compiled bundles are available (plugin browser-client.mjs and browser-service.mjs). browser-service.mjs (1.45 MB) was grepped, not read in full.
- Cloud Browser engine and hosting not identified.
- Shared-drive directory listings timed out several times. webview/assets was counted, not enumerated. native-menu-locales was not enumerated.
- Not inspected by design: credential, cookie, password, history and session stores; the passwords and history settings pages; user data directories.
- Presence of strings is not proof of default enablement or runtime reachability. Feature-gate defaults were checked only where noted.
- No secret values were seen or recorded.

## 7. Pointers for the deep-dive agent (priority order)

1. Browser host, session and route model: .vite/build/main-BklS_2Y2.js (getBrowserHostManager, getBrowserSessionRegistry, persist:codex-browser-app-route, thread-browser-tabs-v1).
2. Browser-use API implementation: PLUG/browser/scripts/browser-client.mjs and browser-service.mjs, checked against PLUG/browser/docs/api.json.
3. Approval and safety enforcement: approval_mode defaults, assertBrowserUseGlobalPersistentApprovalAllowed, navigation restriction, WebMCP origin resolver.
4. Page preload and annotation runtime: .vite/build/browser-page-preload.js (Runtime.addBinding, codex_desktop:browser-page-event).
5. Cloud Browser path and handoff: cloudBrowserUse, requestManualHandoff, botDetection telemetry.
6. Cloud portal tunnel and destination allowlist: cloud-portals classes in main-BklS_2Y2.js and INSTALL/resources/masque-proxy-origins.txt.
7. node_repl sandbox and host services: NODE_REPL_* variables, cua_node/bin/node_repl.exe, cua_repl MCP configuration.
8. Computer-use native pipe and capture worker: main-BklS_2Y2.js, worker.js, INSTALL/resources/native/computer-use-app-icons.node.
9. Import pipeline (cookies, passwords, history, extensions): main-BklS_2Y2.js import schema. Flag as sensitive; review defaults.
