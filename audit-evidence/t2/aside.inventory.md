# Aside browser 1.0.928.2 - inventory (scout pass, read-only)

Scope: installed program files only - C:/Program Files/Aside/Application/1.0.928.2 and C:/Program Files (x86)/Aside. No per-user AppData data was opened. Nothing was launched, installed or executed. Scratch derivatives are listed in Appendix B.
Role: inventory scout (mechanical breadth pass). The deep-dive agent should re-verify anything tagged [L].

Provenance tags: REA = REA analysis of a shipped artifact. SRC = read directly from installed files (grep and byte scans count as SRC). DOC = documentation (none shipped beyond manifests and UI strings). INF = inference (basis given). Confidence: [H] high, [M] medium, [L] low.
Important: en-US.pak strings are mostly upstream Chromium strings with the brand replaced. Presence of a string is not proof that the feature is enabled.

Path shorthand:
- APP = C:/Program Files/Aside/Application/1.0.928.2
- AM = APP/AsideAgentManager (MV3 extension "Aside Browsing Agent")
- PM = APP/AsidePasswordManager (MV3 extension "Aside Vault")
- DMN = APP/AsideDaemon/win-x64 (daemon, Node single-executable app)
- SKL = DMN/static/skills/builtin (32 built-in skills)
- BLOB = scratchpad/tmp/aside/daemon_sea_blob.bin (copy of the daemon's embedded JS payload; never executed)

---

## 0. Scout summary (read first)

- Aside is a Windows x64 fork of Chromium 153 (Chrome/153.0.8010.53, branch-head 8010) published by "At Your Side Inc." The browser core is Aside.dll (306.7 MB). [REA+SRC]
- Three AI layers ship in this install:
  (a) AM: MV3 "Aside Browsing Agent" extension - side-panel agent UI, broad chrome.* automation, chrome.debugger (CDP), tab capture, lasso, remix, translate and page-selection helpers.
  (b) DMN: "Aside Daemon", a Node.js single-executable app (131.7 MB; embedded JS about 14.2 MB). It carries an agent runtime with a QuickJS REPL, 32 built-in skills, routines, heartbeats and webhooks, subagents, memory, an MCP client, about 30 model-provider endpoints, and connectors for Slack, Telegram, Discord, Gmail, Notion, Google Docs and YouTube.
  (c) Computer-use helper (AsideComputerUse, MSIX, runFullTrust) with an on-device Windows AI and ONNX Runtime stack, plus a Windows AppContainer sandbox helper. [SRC/REA]
- PM: "Aside Vault" password manager extension with a Bitwarden-like feature set (passkeys/FIDO2, TOTP, attachments, breach check, card autofill with brand images; import-source names in PM code: 1Password, Bitwarden, Dashlane, LastPass, Proton Pass). Hit counts in PM background.js: passkey 132, totp 128, fido2 27, attachment 24. The literal "collection" has 0 hits, so no collections claim is made; "send" (53 hits) is too generic to claim a Send feature; Apple Passwords artwork sits in AM assets and is not verified in PM code. [SRC]
- Browser UI (Aside.dll C++ views plus en-US.pak) covers tab groups, tab search and switcher, new-tab cards, side panel, Gemini-in-Aside (Chromium Glic) strings, Google account sync and Google Password Manager strings, an ad-block service, importers, profiles and an updater. [SRC]
- Open unknowns: which feature flags are on, the native-messaging host name, the daemon-to-extension wire protocol, and exact tool schemas. These need deeper static or dynamic work. [INF]

---

## 1. Identity

### 1.1 Product and versions
- Product "Aside"; CompanyName "At Your Side Inc."; copyright 2026. [REA: Aside.exe RT_VERSION; re-checked this pass with file VersionInfo: Aside.exe, Aside.dll and aside_proxy.exe all report FileVersion 1.0.928.2 and CompanyName "At Your Side Inc."] [SRC]
- Browser version 1.0.928.2 (Aside.exe, aside_proxy.exe, elevation_service.exe, elevated_tracing_service.exe, aside_migrator.exe, aside_pwa_launcher.exe). [REA+SRC]
- Chromium base: Chrome/153.0.8010.53 (UA string in Aside.dll); LastChange stamp refers to refs/branch-heads/8010. [SRC]
- Daemon: product "Aside Daemon", FileVersion 1.26.928.1922, InternalName "node" (Node single-executable build). Bundle entry path: apps/daemon/build/sea/index.mjs inside a monorepo named "bro-components". Not a .NET assembly (REA inspect_managed_artifact: not-managed, no CLI header); NODE_SEA_FUSE marker present (1 hit). [REA+SRC]
- Extensions: "Aside Browsing Agent" (AM) and "Aside Vault" (PM), both 1.26.928.1922 (manifest.json). [SRC]
- Computer-use helper: "Aside Computer Use" (AsideComputerUse.exe); MSIX identity AtYourSide.AsideComputerUse 1.26.928.1922; publisher "CN=AT YOUR SIDE INC, O=AT YOUR SIDE INC, L=San Francisco, S=California, C=US"; capabilities runFullTrust and unvirtualizedResources; TrustLevel mediumIL; TargetDeviceFamily Windows.Desktop 10.0.19041.0 minimum. [REA+SRC]
- Sandbox helper: "Aside Windows Sandbox Helper", FileDescription "Sandbox for Aside Browser", 1.26.928.1922. [REA+SRC]
- Updater: "Aside Updater (x64)" 1.0.928.2. AsideUpdater/1.0.928.2/updater.exe and Update/AsideUpdate.exe are byte-identical (same full SHA-256, f31be19404ac5ffdfc59ab2293194f6eade7fcb3c9c6377149d5aa7a38dcb2c6, verified this pass). [SRC]
- Chromium component manifests: IwaKeyDistribution 2026.8.5.0; MEI Preload 1.0.7.1652906823; Privacy Sandbox Attestations 2025.7.18.0. [SRC]
- Channel names in strings: Aside Beta, Aside Dev, Aside Canary. [SRC: en-US.pak]
- Platform: Windows 10 or later, x64 only in this install (accept_arch x64 in DMN/manifest.json). [SRC]

### 1.2 Tech stack
- Browser shell: C++ Chromium 153 (Aside.dll); ANGLE (libEGL.dll, libGLESv2.dll); Vulkan loader with SwiftShader; ICU 78 (type names in aside-native.node); V8 snapshot blobs; PDF code (CPDF_* symbol names in aside-native.node). [SRC]
- Windows UI: WinUI 3 (Microsoft.ui.xaml.dll about 15 MB with FileVersion 3.2.0.2511 and ProductVersion 2.0; Microsoft.UI.Xaml.Controls.dll); Windows App SDK 2.0 (Microsoft.WindowsAppRuntime.dll and DeploymentAgent.exe report FileVersion 2.0; RestartAgent.exe present, version not read; computer-use/ also carries a Microsoft.WindowsAppRuntime.Release!2.1.3 marker, so the runtime version is not uniform across the install); composition DLLs (DwmSceneI, wuceffectsi, Microsoft.UI.Composition.OSSupport). [SRC]
- Daemon: Node.js single-executable app. Built-ins used: node:sea, node:sqlite, worker_threads, child_process, http, http2, https, net, dns/promises, zlib, crypto, async_hooks, readline. The bundle is a rolldown-style ESM with import_* identifiers. [SRC]
- Native addons: aside-native.node (7.5 MB; C++/WinRT; Windows Hello KeyCredentialManager and UserConsentVerifier; WinUI dialog code). moss-core.node (15.3 MB; Rust via napi; cargo registry paths show regex-automata 0.4.14, serde_json 1.0.150, tokenizers 0.23.1; strings for embedding models, vector index and tensor memory tags). [SRC; the engine label is INF]
- Extensions (AM and PM): MV3 service workers (type module); Vite/Rolldown chunks (rolldown-runtime chunk in PM assets); React (react.dev/errors literal in PM) [M]; Framer Motion (AnimatePresence chunk name in AM) [M]; Shiki (theme and language chunk names) [M]; Mermaid (diagram chunk names) [H]; Base UI (base-ui.com literal in PM) [M]; tRPC (issue-link literal in PM) [M]; Tailwind (tailwindcss.com literal in PM) [L]; Zod-style schema internals (type:"pipe", "nonoptional", "prefault" literals in AM background.js) [INF, M]. [SRC]
- Daemon JS sources (paths in BLOB): TypeScript; QuickJS-based REPL (scripts/build-quickjs-buffer.ts); browser-v3 injected world (src/browser-v3/injected/aside-world.ts). [SRC]

### 1.3 Dependency list (grouped by capability)
Daemon bundle (import specifiers and import_* identifiers; SRC unless noted):
- Runtime and built-ins: node:sea, node:sqlite, node:worker_threads, node:child_process, node:http, node:http2, node:https, node:net, node:dns/promises, node:zlib, node:stream, node:crypto, node:async_hooks, node:readline, node:os, node:url, node:timers/promises, node:module.
- HTTP and sockets: undici (import_undici; fetch); ws (import_websocket, websocket_server, subprotocol, permessage_deflate, sender, receiver); content-type; mime-types; node-domexception.
- Validation and schema: ajv and ajv-formats (ajv/dist/runtime/*). Zod-style validators are INF [M].
- Search and index: lunr (import_lunr); moss-core (native Rust, vector/embedding index).
- Markup and content: turndown (import_turndown_cjs; HTML to Markdown); png (import_png); canvas (import_canvas; raster, purpose INF); ignore (gitignore-style matching); extend.
- Time: dayjs with plugins relativeTime, isoWeek, isBetween, duration, localizedFormat, updateLocale, timezone, utc.
- Processes and files: cross-spawn; proper-lockfile; yauzl (zip reading); p-retry.
- Channel SDKs: discord.js (packages/discord.js; DiscordjsError and related); Slack (github slackapi/node-slack-sdk; SlackError, SlackMessage); Telegram Bot API over fetch (telegramFetch); Notion (notionToMarkdown); YouTube transcript extractor (YoutubeExtractor, YoutubeTranscriptError); GitHub Copilot chat client (GitHubCopilotChat).
- Google: google-auth-library-nodejs and gaxios (vendored under packages/); google-docs-driver (internal package).
- Model-provider layer (pi-style registry; provider files radius.ts, openrouter.ts, openai-codex.ts, meta.ts, kimi-coding.ts, github-copilot.ts, xai.ts, anthropic.ts, bedrock-converse-stream.ts). API-type identifiers: openai-completions, openai-responses, openai-codex-responses, anthropic-messages, bedrock-converse-stream, mistral-conversations; Vertex and Gemini generativelanguage v1beta also present. Name-level SDK error classes present: OpenAIError, AnthropicError; Mistral:: class names also present. [SRC; library identity INF]
- Telemetry: posthog (host us.i.posthog.com). [SRC]
- Internal workspace packages (paths): packages/native-helper-binding (prebuilds); apps/native-helper/computer-use-helper; apps/native-helper/credential-exchange (macOS bundle name AsideCredentialExchange.app); packages/google-docs-driver. [SRC]
- Fine-tuning type names present in the bundle (SUPERVISED_FINE_TUNING, REINFORCEMENT_TUNING, PREFERENCE_TUNING). Relevance low [L]. [SRC]

Extension bundles (AM and PM; SRC from asset names and literals):
- UI: React, Framer Motion, Shiki, Mermaid, Base UI, tRPC client (see 1.2 for tags).
- Crypto and security (PM): libsodium-wrappers; kdf module; password-generator; fido2-page-script.js (WebAuthn); breach check against api.pwnedpasswords.com (range API).
- Payments: Stripe.js (js.stripe.com/v3 literal in AM).
- Fonts and sounds: Geist (PM); AsideDisplay variable font (both extensions); Google Fonts css2 link (AM); notification.wav and success.wav (sound patch under MIT; copied into AM/sounds).

Browser binaries and components:
- Chromium: resources.pak; chrome_100_percent.pak; chrome_200_percent.pak; icudtl.dat; snapshot_blob.bin; v8_context_snapshot.bin; chrome_elf.dll; chrome_wer.dll; eventlog_provider.dll; Installer (chrmstp.exe, setup.exe); IwaKeyDistribution; MEIPreload; PrivacySandboxAttestationsPreloaded. [SRC]
- Windows and other: WebView2Loader.dll (Microsoft Edge Embedded Browser WebView Loader 1.0.3719.77 per file VersionInfo; use inside Aside not verified [L]); DirectML.dll and DWriteCore.dll (computer-use directory); msdia140.dll (Microsoft DIA SDK); MRM.dll; Microsoft.Security.Authentication.OAuth.winmd (computer-use directory); Microsoft.Graphics.Imaging*.winmd (WinRT imaging metadata). [SRC]

---

## 2. Directory and module map (top 2-3 levels)

C:/Program Files/Aside/
- Application/
  - Aside.exe (4.5 MB) - main launcher. Resources: file-type icons including HTML and PDF; GoogleUpdate command registration; a Windows LimitedAccessFeature token named IDENTITY. [REA]
  - aside_proxy.exe (2.7 MB) - proxy and launcher (InternalName aside_proxy). [REA+SRC]
  - SetupMetrics/ - empty.
  - 1.0.928.2/ - versioned payload:
    - Aside.dll (306.7 MB) - Chromium 153 browser core.
    - resources.pak (22.9 MB) - Chromium WebUI resources.
    - aside_resources.pak (0.9 MB) plus aside_resources.pak.info - Aside resource IDs: aside_importer WebUI (index.html, app.js) and browser icons for Arc, Atlas, Chrome, Comet, Dia, Edge, Firefox and Safari. [SRC]
    - chrome_100_percent.pak, chrome_200_percent.pak - scale image packs.
    - icudtl.dat (ICU data); snapshot_blob.bin and v8_context_snapshot.bin (V8 snapshots).
    - Locales/ - 55 base locale packs, counted this pass (en-US.pak carries the Aside UI strings) plus FEMININE, MASCULINE and NEUTER variants.
    - resources/accessibility/ - one file, reading_mode_gdocs_helper_manifest.json (Reading Mode Google Docs helper, version 153.0.8010.53). Its "key" field is secret-bearing and the file was skipped (see Section 6).
    - GPU and shader stack: libEGL.dll, libGLESv2.dll, vk_swiftshader.dll (+ vk_swiftshader_icd.json), vulkan-1.dll, VkICD_mock_icd.dll, VkLayer_khronos_validation.dll, dxcompiler.dll, dxil.dll, d3dcompiler_47.dll, dcompi.dll.
    - Helpers: notification_helper.exe, elevation_service.exe, elevated_tracing_service.exe, aside_migrator.exe ("Aside Data Migrator"), aside_pwa_launcher.exe, DeploymentAgent.exe and RestartAgent.exe (Windows App SDK 2.0), eventlog_provider.dll, chrome_elf.dll, chrome_wer.dll, WebView2Loader.dll.
    - WinUI and Windows App SDK: Microsoft.UI.* (dll and pri), Microsoft.ui.xaml.dll, Microsoft.UI.Xaml.Controls.dll, Microsoft.UI.Xaml.Internal.dll, Microsoft.UI.Xaml.Phone.dll, WinUIEdit.dll, Microsoft.WindowsAppRuntime.dll, Microsoft.Windows.ApplicationModel.Resources.dll, Microsoft.InputStateManager.dll, Microsoft.Internal.FrameworkUdk.dll, Microsoft.Graphics.Display.dll, Microsoft.DirectManipulation.dll, Microsoft.UI.Input.dll, Microsoft.UI.Windowing(.Core).dll, CoreMessagingXP.dll, MRM.dll, DwmSceneI.dll, dwmcorei.dll, wuceffectsi.dll, marshal.dll (role unknown), msdia140.dll, PushNotificationsLongRunningTask.ProxyStub.dll.
    - Microsoft.UI.Xaml/Assets/ (2 files); en-us/ (MUI resources); 86 language folders at the version root (WinUI and Windows App SDK resources), and the same 86 under AsideDaemon/win-x64/computer-use/ (counted this pass).
    - Installer/ - chrmstp.exe and setup.exe (both 6.2 MB).
    - IwaKeyDistribution/ - manifest.json; iwa-key-distribution.pb (not decoded).
    - MEIPreload/ - manifest.json; preloaded_data.pb (not decoded).
    - PrivacySandboxAttestationsPreloaded/ - manifest.json; privacy-sandbox-attestations.dat (not decoded).
    - AsideAgentManager/ (AM, below).
    - AsidePasswordManager/ (PM, below).
    - AsideDaemon/win-x64/ (DMN, below).
- Program Files (x86)/Aside/
  - AsideUpdater/ - updater root: 1.0.928.2/updater.exe (9.8 MB); Crashpad/ (empty subfolders); uninstall.cmd (removes the updater folder after a path check); crx_cache/ (222 MB hashed CRX cache plus metadata.json); prefs.json; updater.log and updater_history.jsonl (runtime logs, not read).
  - Update/AsideUpdate.exe (same binary as updater.exe).

AsideAgentManager/ (AM):
- manifest.json - permissions, commands, CSP, externally_connectable (1 ID), host_permissions (<all_urls> and https://api.anthropic.com/*), content scripts, web-accessible resources (verified this pass).
- background.js (352 KB) - service worker: chrome.* automation, message hub, lasso, remix, translate, page-selection, capture, native connection (chrome.runtime.connectNative), account API.
- content/content.js (all URLs, document_idle); content/google-docs-selection-bridge.js (docs.google.com/document/*, MAIN world).
- sidepanel.html (side panel shell); main.html (shell page, also web-accessible to api.aside.com and api.asidehq.com origins); iframe.html (generic iframe host). sidepanel.html, main.html and account-password.html are all 4,713 bytes, so they are probably one shared shell page [INF].
- newtab.html (14 KB) and newtab-theme.js - new-tab override.
- tabsearch.html; tab-preview-player.html and tab-preview-player.js (125 KB) - tab search bubble and preview player.
- minipopup.html and minipopup-options.html - mini popup.
- notification.html; account-password.html.
- react-grab-bridge.js (444 KB) - web-accessible to all URLs.
- remix.js (14.6 KB) - remix helper (storage namespace aside.remix.v1).
- aria-snapshot-fallback.js (fallback for accessibility snapshots).
- assets/ - 1,114 JS chunks plus fonts, webp, avif and webm images (UI, Shiki, Mermaid, feature chunks; see Section 3).
- sounds/ (notification.wav, success.wav); hero-illustration/captures/; icons/ (16, 48 and 128 px PNG; brand SVG and PNG).

AsidePasswordManager/ (PM):
- manifest.json - MV3; permissions include webRequest, unlimitedStorage, clipboardWrite and offscreen; commands; content scripts on http, https and file URLs (all frames, document_start); CSP wasm-unsafe-eval.
- background.js (1.8 MB) - vault logic and crypto; sync not verified.
- content/content.js; content-runtime.js (504 KB); autofill-page-script.js (1.4 KB); fido2-page-script.js (18 KB).
- popup.html; iframe.html; offscreen.html.
- assets/ - popup and globals bundles (JS and CSS); libsodium-wrappers; kdf; password-generator; card brand images (Amex, Diners, Discover, Mastercard, UnionPay, Visa); Geist and AsideDisplay fonts; PNG "let-aside-sign-in-for-you".
- icons/.

AsideDaemon/win-x64/ (DMN):
- aside-daemon.exe (131.7 MB) - Node single-executable app. Carries NODE_SEA_BLOB (14.18 MB; copied to scratch as BLOB).
- manifest.json - name "Aside Daemon", version 1.26.928.1922, x64, win.
- computer-use/ - AsideComputerUse.exe (3.6 MB, MSIX app); AsideComputerUse.msix; AsideComputerUse.ilk (19.8 MB link file shipped); Assets/; MSIX/; DirectML.dll (18.7 MB); DWriteCore.dll; Microsoft.Graphics.* WinRT metadata; EventLog-Instrumentation.man; Windows composition DLLs; an on-device Windows AI and ONNX stack (14 Microsoft.Windows.AI* files covering Text, ContentSafety, Imaging, MachineLearning and AICapabilities; onnxruntime.dll 21.7 MB; NPUDetect.dll; six workloads*.json files); Microsoft.Windows.Widgets.dll (2.5 MB); Vision, Media.Capture, Storage.Pickers and Security.Authentication.OAuth WinRT metadata (verified this pass).
- native/ - aside-native.node (7.5 MB); moss-core.node (15.3 MB); Microsoft.WindowsAppRuntime.Bootstrap.dll.
- sandbox/ - AsideWindowsSandboxHelper.exe (0.4 MB).
- static/skills/builtin/ - 32 built-in skill folders (Section 3, taxonomy 6).

Daemon source tree (build-path strings in BLOB; SRC):
- apps/daemon/src/skills/builtin; src/skills/repl-extensions/google-docs; src/browser-v3/injected/aside-world.ts; src/browser-v3/injected/aria-snapshot/{accname, generate-accessibility-tree, capture-redaction, utils}.ts; scripts/build-quickjs-buffer.ts.
- packages/{google-auth-library-nodejs, gaxios, native-helper-binding, google-docs-driver, discord.js}.
- apps/native-helper/{computer-use-helper (windows prod and dev builds), credential-exchange}.

---

## 3. Feature candidates (by taxonomy)

Format: - [provenance][confidence] feature - pointer.

### Taxonomy 1 - Acquisition API surface
- [SRC][H] Agent tools named webfetch and websearch - BLOB (literal tool names).
- [SRC][H] Site-specific automation playbooks (16 folders): airtable, amazon, asana, clickup, confluence, discord, github, google-calendar, google-drive, google-forms, google-slides, jira, linear, linkedin, notion, trello - SKL/site-specific/*.
- [SRC][H] Google service skills: google-search, google-docs, google-sheets, google-gmail, google-accounts - SKL/*; Google Docs REPL extension - BLOB path src/skills/repl-extensions/google-docs.
- [SRC][H] YouTube transcript extraction - SKL/youtube; YoutubeExtractor and YoutubeTranscriptError in BLOB.
- [SRC][M] Notion read path - SKL/notion; notionToMarkdown in BLOB.
- [SRC][H] Image search - SKL/image-search; "Image Search" and "Search this screenshot" strings in en-US.pak.
- [SRC][M] Channel history, thread and user lookup over Slack, Discord and Telegram - SKL/channel.
- [SRC][L] Literals "/sitemap/sitemap.xml" and "search_netindex" in BLOB; purpose unverified.
- [SRC][M] Acquisition mostly happens inside the browser (chrome.tabs, captureVisibleTab, CDP) rather than by server-side crawling; see taxonomies 3 and 9.
- Not observed: standalone crawl, map, batch or hosted extract endpoints (INF: the capability lives in the local daemon).

### Taxonomy 2 - Output formats and content processing
- [SRC][H] HTML to Markdown conversion - turndown in BLOB; BLOCK_LEVEL_ELEMENTS, INLINE_ELEMENTS, FOOTNOTE_LIST_SELECTORS, CONTENT_ELEMENT_SELECTOR constants.
- [SRC][H] Document skills: pdf, docx, pptx, xlsx (create and edit) and draft-preview - SKL/*.
- [SRC][M] PDF handling in the browser: "PDFs automatically get converted so you can search and select text"; "Set Aside as your default PDF viewer"; "Text Annotations tool" (en-US.pak); CPDF_* symbols in aside-native.node.
- [SRC][H] Image generation and editing with reference images (imagegen.generate; returns artifact paths) - SKL/imagegen.
- [SRC][M] Screenshot capture UI - AM chunks capture-toolbar, capture-anchor, capture-result, camera; APIs tabs.captureVisibleTab and tabCapture.getMediaStreamId; "Search this screenshot" (en-US).
- [SRC][H] Diagram rendering (Mermaid): chunk families for about 28 diagram types in AM/assets (flowchart, sequence, gantt, class, state, er, journey, pie, gitGraph, c4, requirement, quadrant, sankey, xychart, venn, wardley, railroad, ebnf, abnf, block, architecture, usecase, swimlanes, eventmodeling, cynefin, ishikawa, info).
- [SRC][H] Code highlighting (Shiki): theme chunks (ayu, catppuccin, and others) and about 200 language chunks in AM/assets.
- [SRC][M] Markdown preview and artifacts: AM chunks markdown-preview, files-artifact, artifact-test, highlighted-body.
- [SRC][M] JSON Schema validation for tool and structured output (ajv in BLOB); record_summary tool.
- [SRC][M] Image decode and encode: png and canvas (BLOB).
- [SRC][M] Page translation: translate:page-start, translate:page-status, translate:page-cancel, translate:page-dismiss, translate:selection messages (AM background.js); translate-status chunk; "Translated to" (en-US).
- [SRC][L] Live Caption and caption translation (en-US; Chromium-derived).
- [SRC][M] Favicons: chrome.favicon permission; www.google.com/s2/favicons literal (AM).
- Not observed: change tracking or diffing; schema-driven page extraction API as a product feature (INF: ajv is internal).

### Taxonomy 3 - Rendering and browser engines
- [SRC][H] Chromium 153 engine with ANGLE and SwiftShader fallbacks (Aside.dll, libGLESv2.dll, vk_swiftshader.dll).
- [SRC][H] CDP access: chrome.debugger.getTargets (12 references across AM files); identifier cdp_exports in BLOB.
- [SRC][H] Page-world and isolated-world injection: aside-world.ts (BLOB path); PM autofill-page-script.js and fido2-page-script.js; AM google-docs bridge in MAIN world.
- [SRC][H] Accessibility-tree snapshots with redaction: generate-accessibility-tree.ts, accname.ts, capture-redaction.ts (BLOB); CAPTURE_REDACTED constant; visual-browse skill refers to snapshot(page), refs and locators.
- [SRC][M] DOM readiness and waits: DOM_READY_TIMEOUT_MS, DEFAULT_WAIT_TIMEOUT_MS, MUTATION_OBSERVER, KEY_LISTENERS (BLOB).
- [SRC][H] Profiles, guest mode and managed profiles: Aside.dll tokens AsideProfile* (avatar, menu edit and delete, attributes updater); en-US profile strings; aside_icon_picker_view.
- [SRC][H] Cookies and storage tools: chrome.cookies.getAll; chrome.browsingData.remove; AsideCookieSyncService and AsideWebsiteStorage* tokens.
- [SRC][H] Login and auth handling: PM autofill (login, signup password refs); passkeys via fido2-page-script.js; account-password gate (AsideAccountPasswordGate and related tokens; account-password.html); Windows Hello in aside-native.node; Google sign-in and Google Password Manager strings (en-US).
- [SRC][M] Proxy handling: http_proxy, https_proxy, no_proxy (names only, BLOB).
- [SRC][M] Locale support: 55 Chromium locale packs; dayjs locale plugins (BLOB).
- [SRC][M] Downloads API: chrome.downloads.download, pause, resume, cancel, erase, search (AM).
- [SRC][M] Preloading and prerender: AsideNtpPrerender, aside_ntp_prerender_service; en-US speculative preload strings (ids 819-822): when a site asks to privately preload links, Aside uses Google servers, which learn which sites get preloaded; cookies may be used when preloading [SRC].
- Not observed: a separate headless browser package (no playwright or puppeteer in the BLOB import list); stealth or fingerprint randomization; geolocation spoofing.

### Taxonomy 4 - Anti-bot, proxies and reliability
- [SRC][H] CAPTCHA-solving skill: SKL/captcha-solver - REPL global captcha with click, drag and readText; auto-injects on reCAPTCHA, Turnstile, hCaptcha and "verify you are human" keywords. Described here as a capability only; policy compliance was not evaluated.
- [SRC][H] Ad and tracker blocking: AsideAdBlockService, AsideAdBlockCosmetic, AsideAdBlockNavigationThrottle (Aside.dll); aside_adblock_render_frame_observer and aside-adblock-manual-dir (lowercase tokens); AM permission at.studio.Aside.ext.private.adblock; en-US "Intrusive ads" category.
- [SRC][M] Safe Browsing and Enhanced Safe Browsing; dangerous-download blocking (en-US; Chromium-derived).
- [SRC][M] Retry and timeout handling in daemon: RETRY_CONFIG, p-retry, FETCH_TIMEOUT_MS, ASIDE_API_SLOW_ENDPOINT_TIMEOUT_MS; TOO_MANY_REQUESTS (429) and PAYMENT_REQUIRED (402) constants; UND_ERR_* mapping.
- [SRC][M] Caching: CACHE_TTL_MS and CACHE_EXPIRES_MS (BLOB).
- [SRC][M] Tab resource control: chrome.tabs.discard (AM); en-US "Aside recommends pausing the tab slowing your browser".
- [SRC][L] Password breach check via the k-anonymity range API (api.pwnedpasswords.com/range) in PM.
- Not observed: residential or rotating proxy pools; robots.txt handling (not searched exhaustively [L]); a CAPTCHA service other than the skill's own mechanism.

### Taxonomy 5 - Orchestration and scale
- [SRC][H] Routines (scheduled and triggered agent runs): RoutineTable, routines, routine_update, routineTrigger, routine_id, session_routine (BLOB); AM chunk routines.
- [SRC][H] Cron and heartbeat: "cron" (31 hits) and Heartbeat (28 hits) in BLOB (names and classes; semantics unverified [M]).
- [SRC][H] Inbound webhooks: WebhookType, WebhookEventType, WebhookEventStatus, webhookVerifier, webhookSecret, webhookConfig; Telegram unverified-webhook flag (name only).
- [SRC][H] Background tasks: run_in_background; task fields task_card, task_display_mode, task_title, task_type, task_update; AM chunks tasks, task-detail, task-error; Aside.dll views AsideTaskPopover, AsideTaskRowButton, AsideTaskStatusDotView, AsideTaskCompletedIndicator, AsideTaskAttentionBadgeView, AsideTaskManagerIcon.
- [SRC][H] Subagents: tools subagent and subagent_wait; SUBAGENT_PROFILES, subagent_profile, SUBAGENT_DIRECTIVE; AM chunks subagent-icon and parent-session-back-button.
- [SRC][M] Session model: session_runs, session_turns, session_tabs, session_thread_message_sent and _received, session_start_limit, session_run_memory_extractions, session_completed, session_stopped; agent_start, agent_end, agent_error events.
- [SRC][M] Shared sessions: MAX_SHARED_SESSION_MESSAGE_TEXT_LENGTH (BLOB); AM chunks remote-sessions and session-host.
- [SRC][M] Browser-side scheduling: chrome.alarms (create, get, onAlarm) in AM; tabs.tidy and extension.history.cleanup events in AM background.js.
- [INF][L] Queueing and worker distribution: not observed; one daemon process with helper processes (computer-use, sandbox, password importer).
- [SRC][M] Budgets and limits: ADD_ON_CREDIT_MAX_CHARGE_CENTS; subscription-usage chunk; session_start_limit.

### Taxonomy 6 - Platform and ecosystem
- [SRC][H] Skills system: 32 built-in skills in SKL (aside, bitwarden, 1password, lastpass, dashlane, proton-pass, password-manager, captcha-solver, channel, chrome, visual-browse, imagegen, image-search, google-accounts, google-docs, google-gmail, google-search, google-sheets, kakaotalk, notion, notification-activation, onboarding, pdf, docx, pptx, xlsx, draft-preview, site-specific, skill-creator, slack, x-twitter, youtube). Account and user skills: GENERAL_SKILLS_DIR, USER_SKILLS_PATH_PREFIX, EPISODIC_DIR (BLOB). SKILL.md frontmatter: name, description, icon, autoInject.keywords, metadata.internal. skill-creator/scripts/validate-frontmatter.mjs.
- [SRC][H] MCP client: McpManager, McpConnectionError, McpStdioConnectionError, McpOAuthProvider, mcpServers and mcp_servers, mcpTools, MCP_TOOL_LIST_MAX_TOOLS, MCP_TOOL_LIST_MAX_PAGES, MCP_TOOL_LIST_TOTAL_TIMEOUT; events mcp_tool_use and mcp_tool_result; AM chunk mcps; mcp-tunnels literal.
- [SRC][M] Extension bridge to the browser: ExtensionBridgeProtocol, ExtensionBridgeCommand, extensionBridgeRoute, extensionBridgeRouteResolver, extensionName, EXTENSION_MV constants; AM chunks extension, extension-main, extension-page.
- [SRC][L] Plugin and hook surfaces: plugins (11 hits), Hook (5), hooks (44), "Plugin-Version" header (31) - structure unverified.
- [SRC][H] Provider marketplace and bring-your-own provider: AM chunks custom-provider, provider, provider-context, providers, data-provider, model-select, model-categories, use-default-model; provider IDs ASIDE_PROVIDER_ID, CLAUDE_CODE_PROVIDER_ID, OPENAI_CODEX_PROVIDER_ID, GITHUB_COPILOT_PROVIDER_ID, XAI_GROK_OAUTH_PROVIDER_ID, VERCEL_AI_GATEWAY_PROVIDER_ID, COMMANDCODE_PROVIDER_ID; CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_GATEWAY_ID (names).
- [SRC][M] Coding agents as providers: AM chunk coding-agent-nudge; CLAUDE_CODE_PROVIDER_ID; CODEX_HOME env name.
- [SRC][H] Projects: AM chunk use-move-session-to-project; aside skill description mentions Projects.
- [SRC][M] Teams, orgs and policy: "Aside Enterprise" and managed-profile strings (en-US); account-scoped chunk; session-permission-templates chunk.
- [SRC][H] Billing and monetization: AM chunks billing, subscription, subscription-usage, pricing-table-shared, add-on-credits-dialog; ENTITLEMENT_CREATE and ADD_ON_CREDIT_* constants (BLOB); Stripe.js (AM).
- [SRC][H] Memory system: memory_store_id, memory_stores, memory_files, memory_versions, memory_extraction, memory_signal, memory_search, memory_read, memory_root (BLOB); the password-manager skill cites a memory/USER.md file.
- [SRC][L] Store surfaces: "Aside Web Store" (en-US); chromewebstore.google.com links (AM, PM).
- [SRC][M] Mobile companion: en-US QR-code strings for the Aside iOS app.
- [SRC][M] Upstream Chromium WebUI routes chrome://skills and chrome://settings/ai/skills exist in resources.pak (verified this pass); whether Aside exposes its own skills through them is not verified [INF].
- Not observed: a marketplace of actors or templates like Apify; hosted dataset or key-value storage products.

### Taxonomy 7 - Observability and operations
- [SRC][H] Analytics: PostHog host (BLOB); "analytics" chunk (AM).
- [SRC][M] Crash reporting: Crashpad folder (Program Files (x86)/AsideUpdater/1.0.928.2/Crashpad, empty); en-US strings on sending crash reports to Google and "Crash reporting is not available in Aside."
- [SRC][M] Logs and events: Windows event manifest (computer-use/EventLog-Instrumentation.man); eventlog_provider.dll; elevated_tracing_service.exe (ETW). Updater logs present (not read).
- [SRC][H] Daemon health and lifecycle: AsideDaemonHealthLoader, AsideDaemonShutdownLoader, aside_daemon_shutdown_request, AsideDaemonProcessesForInstallDir, AsideDaemonSessionToken (Aside.dll tokens); DAEMON_VERSION (BLOB).
- [SRC][H] Updates: updater with badge and prompt UI (AsideUpdateBadge*, AsideUpdatePromptBackground); component updater (aside_component_update*, aside_component_crx_cache); STAGING_APP_PATH and PREVIOUS_APP_PATH (BLOB; staging and rollback); "relaunch to update" strings (en-US).
- [SRC][M] Replay and feedback: session_replay and session_replay_event_trigger_activated_ (BLOB); session-feedback-dialog chunk; chrome://chrome-urls debug-page strings (en-US).
- [SRC][M] Usage and cost: subscription-usage chunk; ADD_ON_CREDIT_* constants.
- [SRC][M] Self-hosting and deploy: releases.aside.com/install.sh (AM literal; suggests a script install on other platforms [INF]); RELEASE_BASE_URL (BLOB); MSIX packaging for the computer-use helper.
- [SRC][H] Sandboxing: AsideWindowsSandboxHelper.exe ("Sandbox for Aside Browser"); SandboxBackend with an appcontainer option; SANDBOX_EXEC_PATH and SANDBOX_HELPER_* constants (names).
- [SRC][M] Daemon authorization: aside_daemon_authorizer (Aside.dll token); chrome.asideAccount.signDaemonAuthChallenge (AM).
- [SRC][M] Startup gating: AsideStartupReadinessGate token.
- [SRC][M] Multi-tenancy: profiles; account-scoped chunk and URLs.

### Taxonomy 8 - In-app browser shell (user-facing)
- [SRC][H] Tabs and windows: pinned tabs (AsidePinnedTab*); tab groups (create, rename, share, pin, move to new window, saved groups; AsideTabGroup* tokens; en-US); tab switcher (AsideTabSwitcherView, AsideTabSwitcherItemView); tab search (tabsearch.html; AsideTabSearchBubble and WebView tokens); tab preview (tab-preview-player); "Open in split view" (en-US); tab styling Classic and Modern (en-US); "Show organizer button" (en-US).
- [SRC][H] Address bar (omnibox): AsideOmniboxAPI and aside_omnibox (_api) tokens; AM chunks ask-omnibox-panel, omnibox-scope, omnibox-mode-preference; en-US "@gemini" omnibox entry.
- [SRC][H] New tab page: newtab.html and newtab-theme.js; "Customize Aside"; cards (Top picks, Recently used, Your tab groups, Suggested for you, Calendar meeting cards, SharePoint and OneDrive file cards) (en-US); AsideNewTabPage, AsideNtpTransition tokens.
- [SRC][H] Side panel: sidepanel.html; shortcut Alt+Shift+B (AM manifest); bookmarks, history and reading list in the side panel (en-US).
- [SRC][H] Bookmarks and reading list: AM chunk bookmark; AsideBookmarkShelfIcon, AsideBookmarksSidePanelRefreshIcon; bookmark folders and "Bookmark all tabs" (en-US); reading list (en-US).
- [SRC][H] History: AM chunk history; chrome.history.* APIs; "Show history in side panel" (en-US).
- [SRC][H] Downloads: chrome.downloads.*; AsideDownloadMenuIcon, AsideDownloadInProgress* tokens; dangerous-download blocking (en-US).
- [SRC][H] Extensions UI: AM chunks extension, extension-main, extension-page; AsideExtensionTabPlacement and AsideExtensionsToolbarButtonElementId tokens; chrome.management.*; "Discover extensions"; extension access-request strings and USB or HID access notices (en-US).
- [SRC][H] Vault and password UI: popup.html; keyboard commands Ctrl+Shift+Y (open vault), Ctrl+Shift+L (autofill login), Ctrl+Shift+G (generate password), lock_vault (no default key); AsidePasswordManagerIcon; AsideAccountPasswordMenu; AM chunks password, password-import-menu, import-password; Google Password Manager strings (en-US).
- [SRC][H] Password and data import: AM chunks import, import-browser, import-password, use-browser-importer; aside_importer WebUI (IDR_ASIDE_* resources); AsideMigrator and aside_migrator service tokens; aside-import-data; browser icons for Arc, Atlas, Chrome, Comet, Dia, Edge, Firefox, Safari (aside_resources.pak.info); AM assets for 1Password, Bitwarden, Brave and Apple Passwords imports [M]; importer skills 1password, bitwarden, dashlane, lastpass, proton-pass.
- [SRC][H] Browser import page (aside_resources.pak ids 15140-15141, verified this pass): a one-click Import page titled "Aside Importer" that brings in saved bookmarks, browsing history and passwords. Its script includes importFirefoxData, a Safari import guide with an invalid-file path (safari_import_guide, onSafariImportInvalidFile) and browser-import started, succeeded and failed listeners.
- [SRC][H] Autofill of cards, identities and addresses: AM chunks payment and address-related; PM card brand images; en-US "Manage shopping info", "Autofill just got better", payment-handler strings.
- [SRC][H] Profiles: AsideProfileMenu*, AsideProfileAvatar, AsideProfileIndicator tokens; aside_icon_picker_view; en-US profile creation and guest strings.
- [SRC][H] Sync and Aside account: AM chunks account, account-avatar, account-scoped; AsideAccount* tokens; browser-sync.asidehq.com literal (Aside.dll); en-US passphrase text for account data; send-tab-to-device strings; AsideSync* tokens (sync refresh watchdog, encryption controller, auth manager in lowercase tokens).
- [SRC][H] Aside account in the settings WebUI (resources.pak, verified this pass): tokens isAsideAccountSignedIn, asideAccountEmail, asideLastSyncText, asideSyncDiagnosticText, asideAccountWaitingForFirstSync, asideAccountSettingsUrl and manageAsideAccount, plus about 40 --color-aside-* theme tokens.
- [SRC][H] Upstream Chromium WebUI routes (resources.pak, verified this pass; 5,609 entries, 126 unique page titles, 358 chrome:// route strings): chrome://settings/ai, chrome://settings/ai/skills, chrome://glic, chrome://glic-fre/, chrome://glic-experimental-opt-in/, chrome://contextual-tasks/, chrome://compare/, chrome://graduation/, chrome://omnibox (aim-eligibility, ml), chrome://read-later.top-chrome/, chrome://tab-search.top-chrome/, chrome://customize-chrome-side-panel, chrome://history-side-panel, chrome://bookmarks-side-panel, chrome://password-manager, chrome://password-change, chrome://extensions, chrome://flags, chrome://downloads, chrome://new-tab-page, chrome://profile-picker, chrome://print, chrome://net-export, chrome://policy, chrome://crashes. Titles also include Gemini in Chrome, Remembered Notes, Tab Group Home, Watermark Customization, Adblock Internals and Private AI internals. Many ChromeOS-only pages are also bundled; these are upstream, not Aside-specific.
- [SRC][M] Settings and customization: AM chunks settings, settings-gear, settings-slider, appearance, use-update-settings, use-agent-settings; en-US "Customize Aside", "AI innovations" and "AI in Aside" settings, Labs, Languages, Accessibility, Performance and energy saver, launch at Windows startup, default-browser and default-PDF-viewer prompts.
- [SRC][M] Notifications: AM chunk notification; notification.html; AsideNotifications* tokens; notification_helper.exe; notification.wav and success.wav sounds.
- [SRC][M] Permission prompts: AM chunks permission-approval and permission-selector; site permission strings (camera, microphone, location, Bluetooth, USB, HID, screen share); AsideControl* tokens.
- [SRC][M] Developer tools: AsideDeveloperToolsIcon token; en-US DevTools strings.
- [SRC][M] Reader mode, zoom and print: en-US "Reading mode button on the toolbar"; chrome.tabs.setZoom and setZoomSettings, onZoomChange; AsidePrinterStrokeIcon.
- [SRC][M] Chromium-derived Google services in the browser: Google Lens ("Search with Google Lens", "Image Search"); Translate (translate chunk); Live Caption; Preview (definition on selected text); PDF viewer with annotations (en-US).
- [SRC][M] Gemini in Aside (Chromium Glic): side panel and selection overlay; "Share tab with Gemini"; "Gemini Spark" auto browse (runs on the device when awake and Aside is running); Gemini task banners (en-US). Enablement not verified [L]. en-US.pak: "Gemini" appears in 144 entries and "Gemini Spark" in 12; resources.pak has chrome://glic, chrome://glic-fre/ and chrome://settings/images/gemini_spark.svg (verified this pass) [SRC].
- [SRC][M] AI context-menu actions: "Ask Aside", "Tell me about this", "Explain this selection", "Summarize", "Create image with Gemini" (en-US).
- [SRC][H] Lasso: AM chunks lasso and lasso-shortcuts; messages lasso:transform:start, chunk, complete, error, abort; lasso:get-settings; set-lasso-settings; lasso-search-image; lasso-copy-code (AM background.js).
- [SRC][M] Remix: remix.js; messages remix:prompt, remix:get-enabled, remix:sync (AM). Exact user behavior unverified [L].
- [SRC][M] React component grab: react-grab-bridge.js, web-accessible to all URLs [INF: purpose].
- [SRC][M] Page selection context: messages page-selection-context:get, subscribe, update, event; text-selection (AM background.js).
- [SRC][M] Capture: message capture:open; chunks capture-*; camera chunk.
- [SRC][M] Mini popup: minipopup.html; aside_mini_popup_coordinator token.
- [SRC][M] Onboarding: AM chunks onboarding, onboarding-helper, onboarding-layout, onboarding-field-group; onboarding skill.
- [SRC][L] Windows native caption overlay token (aside_native_caption_overlay_win) - purpose unknown.
- [SRC][M] Tab tidy: tabs.tidy event (AM); en-US "Pause these tabs" suggestion.
- [SRC][M] Windows integration: Mica and acrylic backdrop tokens (AsideWindowsBackdrop, AsideWindows10*); taskbar pin; launch at startup; WinUI task popover (aside_task_popover_win); Windows Hello prompts.
- [SRC][L] Mobile QR code strings (en-US).

### Taxonomy 9 - AI-browser control
- [SRC][H] Browsing agent with side-panel sessions: AM sidepanel and session chunks (session-list, session-composer, session-host, session-files, session-title, new-session, sidebar); Aside.dll AsideAgentSessionTabsView, AsideAgentTabsButton, AsideAgentTabsBubbleView, AsideNewChatTabButton, AsideAiTabsTakeOver, AsideAiTabsViewport.
- [SRC][H] Agent REPL globals (from skill text): chrome (whitelisted MV3 methods), passwordManager, captcha, channel, imagegen, aside, cua (coordinate-based visual browsing), snapshot(page) with refs and locators.
- [SRC][H] Agent tools outside the REPL (BLOB literal names): bash, read_file, write_file, edit_file, grep, glob, terminal, repl, skill, websearch, webfetch, memory_search, history_search, history_read, send_message, ask_user_question, record_summary, get_time, write_todos, subagent, subagent_wait, routine_update.
- [SRC][H] Desktop computer use: AsideComputerUse helper with actions mouse.move, mouse.click, mouse.drag, mouse.context_menu, mouse.wheel and keyboard.text_input, keyboard.press, keyboard.submit, keyboard.shortcut; screenshot pipeline constants (SCREENSHOT_TIMEOUT_MS, SCREENSHOT_TEXTURE_SIZE, ScreenshotOnEverySnapshot, ScreenshotClipToTextureLimit); KEYBOARD_LAYOUT; clipboard filter; computer-use-preview-10-2025 and computerUseToVertex strings (BLOB).
- [SRC][M] On-device Windows AI stack in the computer-use helper (Microsoft.Windows.AI* WinRT metadata and DLLs, onnxruntime.dll, NPUDetect.dll, workloads*.json; verified this pass). Purpose not verified; plausibly local vision, text or content-safety models [INF].
- [SRC][M] Element targeting: ARIA snapshot with accessible names and redaction (BLOB aria-snapshot/*.ts); refs and locators; chrome.debugger for CDP.
- [SRC][H] Action approval and authorization: AM chunks permission-approval, permission-selector, session-permission-templates; AsideTaskCommandAuthorizationFetched and AsideTasksAuthorizationFetched tokens; aside_daemon_authorizer; chrome.asideAccount.signDaemonAuthChallenge (AM).
- [SRC][M] Sandboxed execution: AsideWindowsSandboxHelper (appcontainer backend); QuickJS REPL (scripts/build-quickjs-buffer.ts).
- [SRC][M] User takeover and co-browsing: AM chunks browser-binding, use-browser-broadcast, parent-session-back-button; AsideAiTabsTakeOver token; browserMode and tabs-without-normal-window strings (AM background.js).
- [SRC][M] Multi-tab and multi-session agents: session_tabs, session_runs, subagent profiles, AsideAgentSessionTabsView.
- [SRC][M] Background browsing: run_in_background; Gemini Spark auto-browse constraint strings (en-US).
- [SRC][M] Task record and replay: session_replay events (BLOB); task transcripts (aside skill); session-feedback-dialog chunk.
- [SRC][H] Vault use by the agent: fillCredentials and fillGeneratedPasswordRef messages (AM background.js); password-manager skill says the agent can use Aside Vault without seeing generated passwords; PNG "let-aside-sign-in-for-you" (PM assets); PNG and webp "aside-payment-use" (AM assets).
- [SRC][H] CAPTCHA handling: SKL/captcha-solver (see taxonomy 4).
- [SRC][M] Site-specific playbooks: SKL/site-specific/* (16 folders; see taxonomy 1).
- [SRC][L] Prompt-injection defenses: no dedicated name found in the scanned strings (absence in scanned set only).
- [SRC][L] Domain allowlists: none found by name (absence in scanned set only).
- [SRC][L] Agent cursor or overlay component: not found by name; AsideNativeCaptionOverlay is a caption overlay, not an agent cursor.

### Taxonomy 10 - Integration-plane overlap
- [SRC][H] Chat-platform connectors: SKL/channel (Slack, Discord, Telegram); SKL/kakaotalk; AM chunks channels, channel-platform-icon, chat-list-item, chat-preview-card; bot-token and signing-secret env names (names only); AM literal docs.aside.com/help/channels-*.
- [SRC][L] Weak signals for other chat or collaboration platforms: Teams, SMS, iMessage (iMessagesResponseError), Threads (www.threads.com), x.com, reddit hosts - purpose unverified.
- [SRC][H] Google Workspace: SKL/google-accounts, google-docs, google-sheets, google-gmail, google-search, google-drive, google-calendar, google-forms, google-slides; mail.google.com (AM and BLOB); people-pa.clients6.google.com (BLOB); oauth2.googleapis.com token and revoke endpoints.
- [SRC][H] Notion: SKL/notion; www.notion.so literal; notion-to-md.
- [SRC][H] Developer and work tools: SKL/github (site-specific); GitHub Copilot provider; site-specific jira, confluence, asana, clickup, airtable, linear, trello, linkedin.
- [SRC][M] Social and media: SKL/x-twitter; SKL/youtube; youtube.com and youtube watch literals (BLOB).
- [SRC][H] Password managers as connectors: SKL/1password, bitwarden, dashlane, lastpass, proton-pass, password-manager. The password-manager skill says connected external provider preferences live in memory/USER.md.
- [SRC][M] MCP servers with OAuth: McpOAuthProvider; mcp-tunnels literal.
- [SRC][H] Inbound webhooks and triggers: routines and Telegram, Slack and Discord webhooks (BLOB).
- [SRC][M] OAuth providers seen: Google (oauth2.googleapis.com); GitHub Copilot; OpenAI Codex (chatgpt.com/backend-api); xAI Grok OAuth; PI_OAUTH_CALLBACK_HOST env name.
- [SRC][M] Knowledge and sync: memory stores and history stores; lunr index (BLOB); Aside account browser sync (browser-sync.asidehq.com).
- [SRC][M] Data-connector bridge in the browser: google-docs-selection-bridge (AM content script, MAIN world).

---

## 4. Extension points (plugin, skill, MCP, API, CLI, webhook surfaces)

- Skills: SKILL.md packages in SKL (32) plus account and user skills created by skill-creator. Frontmatter fields: name, description, icon, autoInject.keywords, metadata.internal. [SRC]
- REPL globals and REPL extensions: chrome, passwordManager, captcha, channel, imagegen, aside, cua, snapshot; repl-extensions/google-docs. [SRC]
- MCP servers: stdio and remote; OAuth provider; tool-list paging and caps. [SRC]
- Subagent profiles (SUBAGENT_PROFILES) and slash commands (slashCommandHandlers; AM chunk commands). [SRC]
- Extension bridge routes (ExtensionBridgeRoute, ExtensionBridgeProtocol, ExtensionBridgeCommand). [SRC]
- Chrome extension hosting: chrome.management.* (get, getAll, setEnabled, launchApp, launchExtension); "Aside Web Store" label; chromewebstore.google.com links. [SRC]
- Private extension APIs (manifest permissions; AM lists all eight, PM lists account and adblock): at.studio.Aside.ext.private.account, .adblock, .browser-import, .notification, .omnibox, .pref-get-set, .capture-tab-without-userinteraction, .launch-extension. [SRC]
- externally_connectable IDs: AM accepts clcdgiameigmljcbkkcbjiljinmfkncl; PM accepts fjdhphbdlfjogobdofoaagnlnkoibdge. [SRC]
- AM host_permissions include https://api.anthropic.com/* (direct Anthropic API host access from the extension; verified this pass) [SRC].
- chrome.runtime.onMessageExternal and onUserScriptMessage handlers in AM background.js; userScripts permission declared. [SRC]
- Native messaging: chrome.runtime.connectNative (2 references across AM files); handshake messages register, connectionParams, pong. Host name not recovered. [SRC]
- Daemon local surface: loopback literals 127.0.0.1 (18 hits), port 5175 (3 hits), path /remote-control (1 hit); env names DAEMON_URL, DAEMON_BASE_URL; callback constants CALLBACK_PORT, CALLBACK_PATH; WebSocket server (ws). [SRC]
- Cloud API: api.aside.com and api.asidehq.com (env names ASIDE_API_URL, ASIDE_API_BASE_URL); static.asidehq.com (skill icons); releases.aside.com (release and install). [SRC]
- Webhooks (inbound): routines and Telegram, Slack and Discord. [SRC]
- CLI: none shipped in this Windows install. The AM asset name aside-cli is a marketing image name only [L]. The releases.aside.com/install.sh literal suggests a script install on other platforms [INF].
- Keyboard command surfaces: Alt+Shift+B (side panel); Ctrl+Shift+Y, Ctrl+Shift+L, Ctrl+Shift+G (vault). [SRC]
- Context menus (chrome.contextMenus) and the omnibox private API. [SRC]
- Bring-your-own model providers and custom-provider endpoints (AM chunks; provider IDs). [SRC]

---

## 5. Data and protocol hints

- Storage: SQLite (node:sqlite in BLOB); JSON and JSONL artifacts (SESSION_ARTIFACTS_DIR, SESSION_ATTACHMENTS_DIR); memory files (memory/USER.md, EPISODIC_DIR, memory_root); runtime directories (ASIDE_HOME_PATH, ASIDE_RUNTIME_DIR, RUNTIME_DIR; names only); file locking (proper-lockfile); lunr index (BLOB); moss-core vector index (strings reference a session.json listing vectors); chrome.storage.local (AM, PM); ASIDE_KEYCHAIN_ID_PREFIX (name only). [SRC]
- Wire formats: JSON with ajv schemas; protobuf for Chromium data files (IwaKeyDistribution, MEIPreload; not Aside code); WebSocket (ws with permessage-deflate); HTTP/2 import; gzip and brotli decode (zlib in BLOB). [SRC]
- Extension to daemon: chrome.runtime.connectNative with handshake messages (register, connectionParams, pong) in AM background.js; daemon-side transport not recovered. [SRC/INF]
- Daemon to helpers: child_process with stdio pipes; COMPUTER_USE_HELPER_* and SANDBOX_* path constants; no named-pipe literal found. [SRC]
- Auth: Aside account (api.aside.com) with daemon auth challenge (signDaemonAuthChallenge) and session tokens (AsideSessionToken, AsideSecureToken, AsideDaemonSessionToken - names only); OAuth 2.0 with localhost callbacks (CALLBACK_PORT, CALLBACK_PATH, REDIRECT_URI, AUTHORIZE_URL, TOKEN_URL names; PI_OAUTH_CALLBACK_HOST); JWT claim path constant (JWT_CLAIM_PATH) for ChatGPT or Codex accounts; ed25519 literal (2 hits; use unverified [L]); Windows Hello verification (aside-native.node); Google sign-in (Chromium sync strings). [SRC]
- Sync: browser-sync.asidehq.com (Aside.dll); AsideSyncEncryptionController and passphrase strings (en-US); sync data types AsideWebsiteStorageSpecifics and AsideWebsiteStorageEntrySpecifics; cookie sync service. [SRC]
- Model API surfaces: OpenAI-compatible completions and responses; Anthropic messages; Bedrock converse stream; Mistral conversations; Gemini generativelanguage v1beta; Vertex; about 30 provider hosts (Section 1.3 and taxonomy 6). [SRC]
- Model identifiers seen in strings (catalog names only; currency not verified): claude-sonnet-5, claude-opus-5, claude-code, gpt-5.4, gpt-5.4-mini, gpt-5.5, GPT-5.6 variants (sol, luna, terra), gpt-6-astra, deepseek-v4-pro, computer-use-preview-10-2025, kimi-for-coding, gpt-oss-120b and gpt-oss-20b. [SRC]
- Secret-bearing names (names only; no values recorded): SLACK_ENCRYPTION_KEY, SLACK_SIGNING_SECRET, SLACK_CLIENT_SECRET, SLACK_BOT_TOKEN, TELEGRAM_BOT_TOKEN, TELEGRAM_WEBHOOK_SECRET_TOKEN, DISCORD_BOT_TOKEN, DISCORD_PUBLIC_KEY, webhookSecret, GOOGLE_APPLICATION_CREDENTIALS, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY. [SRC]
- Secret-pattern counts in the daemon SEA blob (counts only; no values recorded or printed): one match of the Google API key shape (AIza plus 35 characters; treat as secret-bearing and unverified); zero matches for AWS access key IDs, Anthropic and OpenAI key shapes, Slack and GitHub token shapes, and PEM private-key headers. [SRC]
- Privacy-relevant UI statements (en-US.pak ids 819-823, verified this pass): spell-check sends the text typed in text fields to Google when enabled; private link preloads go through Google servers, which learn which sites are preloaded. [SRC]
- Telemetry: PostHog. [SRC]

---

## 6. Blockers and limits

- No pre-extracted Aside copy exists in scratchpad/extract/ (only antigravity, claude, codex, grokbot, manus, slack and t3code). I worked from the read-only install folder and made one byte copy of the daemon's embedded payload into scratchpad/tmp/aside/. [SRC]
- Location note: the task listed Aside.exe and aside_proxy.exe under the 1.0.928.2 folder. They sit one level up in C:/Program Files/Aside/Application/ (version-read this pass). [SRC]
- Aside.dll (306.7 MB) was not opened with REA open_binary or search_strings. The REA active target is shared by all agents, and a Chromium-sized module is too slow to load. Strings came from grep byte scans (SRC) and PE resources from inspect_pe_resources (REA). [INF]
- This pass: REA inspect_pe_resources on Aside.dll returned a 74,689-character result, which the tool saved to the Claude session store. Under the data-store rule it was not opened, so the PE resource tree of Aside.dll is not summarized here. [REA]
- REA analyze_javascript_application on AsideAgentManager (summary) completed: 1,133 relevant files, 1,122 parsed JS files, 14,447 findings, 0 modules recovered, 0 Electron surfaces, 2,215 limitations, 2,230 unknowns. The graph is partial. The full result sits in the Claude session store (path not recorded here). This pass did not open it and did not re-run the analysis (one heavy analysis per app). [REA]
- REA inspect_analysis_view rejected the sections collection for JS evidence (valid only for binary-layout evidence). Module pages were not usable. [REA]
- REA trace_application_feature with seed api chrome.runtime.connectNative returned zero matches. This is an incomplete graph, not evidence of absence. [REA]
- The daemon's 14.2 MB embedded JS was not run through REA analyze_javascript_application, to keep to one heavy analysis per app. Recommended for the deep dive: point REA at a directory holding the extracted bundle.
- Feature presence is not feature enablement. en-US.pak strings and Aside.dll tokens show presence only. Chromium base::Feature names were not enumerated systematically; only Aside-prefixed tokens were listed.
- Not read: per-user data (AppData Roaming and Local for Aside, Aside User Data); updater logs and prefs (runtime state); crx_cache (identity only); IwaKeyDistribution, MEIPreload and PrivacySandbox binary data (identity from manifests only); resources/accessibility (except the secret-bearing manifest, which was skipped); locale packs other than en-US. aside_resources.pak was decoded this pass: PNG payloads are stored raw, SVG and HTML/JS are gzip-compressed. Decoded files were not saved to scratch because Python could not write the long scratch path.
- Partial reads: PM background.js (1.8 MB) and AM assets were grepped for names, hosts and API literals only. Daemon bundle was grepped, not read end to end.
- Secrets handling: the AM and PM manifests carry public-key "key" fields (classified as SubjectPublicKeyInfo; values not recorded). resources/accessibility/reading_mode_gdocs_helper_manifest.json has a "key" field that decodes to a PKCS#8 private key: secret-bearing file skipped, value not recorded. An early read in this pass printed that manifest in full before its key type was known; the value was not written to any file or into this inventory. Environment-variable names and constants were recorded; no values were printed or stored. Matches on the vault code were limited to feature words and host literals.
- Heuristics: the daemon tool list comes from quoted literals, not from a schema. The site-specific skill list is complete (16 folders). Mermaid, Shiki, React, Framer Motion and Base UI identifications come from chunk names and literals (INF or M).
- Single-hit literals are flagged [L] wherever they were used.

---

## Appendix A - REA tools actually used

- ToolSearch loaded the REA schemas.
- This pass (after the usage limit): inspect_managed_artifact (ok) on aside-daemon.exe: not-managed, no CLI data directory (sha256 starts 8217fe22). inspect_pe_resources (ok) on AsideWindowsSandboxHelper.exe: RT_MANIFEST and RT_VERSION only, coverage complete. inspect_pe_resources on Aside.dll: 74,689-character result saved to the session store, not opened. Not used this pass: analyze_javascript_application (no second heavy run), inspect_analysis_view, trace_application_feature, open_binary, search_strings, list_strings.
- inspect_pe_resources (ok): Aside.exe; aside_proxy.exe; aside-daemon.exe (max_file_bytes 256 MiB); AsideComputerUse.exe; AsideWindowsSandboxHelper.exe.
- analyze_javascript_application (ok): AsideAgentManager as a directory; detail summary; integrity_policy record-and-continue; max_heap_mb 2048; analysis_timeout_ms 600000.
- inspect_analysis_view (one call rejected: sections collection is invalid for JS evidence).
- trace_application_feature (ok, zero matches): seed api chrome.runtime.connectNative.
- Not used: open_binary, binary_session, binary_overview, search_strings, list_strings (active-target sharing avoided); a second analyze_javascript_application on the daemon (one heavy JS analysis per app).

## Appendix B - Scratch artifacts (scratchpad/tmp/aside/)

- daemon_sea_blob.bin - 14,179,366 bytes; copy of the NODE_SEA_BLOB resource from aside-daemon.exe. Never executed.
- dll_Aside_tokens.txt - 294 unique Aside-prefixed tokens with counts from Aside.dll.
- dll_aside_lower_tokens.txt - 58 lowercase aside-prefixed tokens.
- dll_https_aside.txt - 3 hosts (docs.aside.com, browser-sync.asidehq.com, aside.com/whats-new).
- dll_aside_urls.txt - no aside:// scheme hits.
- msix/ - unpacked AsideComputerUse.msix (AppxManifest.xml with rescap:runFullTrust and rescap:unvirtualizedResources, mediumIL, win32App).
- paks.out - stdout of this pass manifest-key classification and pak parse: en-US.pak summary and 579 "aside" string hits (from line 19), aside_resources.pak summary with importer tokens, resources.pak titles, chrome:// routes and aside tokens.
- pak/ - empty; decoded importer payloads were not written (path-length limit).
