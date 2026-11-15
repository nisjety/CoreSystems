# Microsoft Teams (new client, MSIX) - inventory (teams.inventory.md)

Scout: inventory for the Verevon Space audit (AUDIT 1). Read-only. Date 2026-10-13. Input for a later deep-dive agent: breadth and pointers over depth.
Package: MSTeams 26260.1701.5139.3736, x64, installed read-only at @R.

## 0. Legend, path aliases, coverage

Provenance tags:
- REA = observed through REA analysis of a shipped artifact.
- SRC = read directly from files (manifest, JSON, notices, config, strings pulled with grep -a or Git Bash tools).
- DOC-F = Microsoft Learn page fetched and read in this run.
- DOC-S = public search snippet or secondary source (page not opened); lower confidence.
- INF = inference; the basis is stated.

Path aliases used below (the @ stands for a path root):
- @R = C:/Program Files/WindowsApps/MSTeams_26260.1701.5139.3736_x64__8wekyb3d8bbwe (read-only).
- @T = scratch folder scratchpad/tmp/teams. It holds outputs from an earlier pass of this same scout (en_us_keys_flat.txt, namespaces_all.txt, feature_kw_counts.txt, infra_counts.txt) plus this run's helper files.
- @K = @T/en_us_keys_flat.txt: 16,366 en-us string keys (16,120 unique), from main-locale-en-us-23ba9acb36586a4b.js.gz inside @R/desktop-assets/hashed-assets. Prefix counts use the first underscore token.
- @NS = @T/namespaces_all.txt: 124 namespaces found by scanning 6,902 .js.gz chunks. Noisy: includes code identifiers (keyword, root, svg namespaces). Use as a pointer list, not as exact counts.
- @AM = @R/desktop-assets/asset-manifest.json: 7,014 entries; 5,209 unique named stems after removing hashes and trailing digits; 1,019 entries are numeric-only chunk names.
- @SCAN = this run's scan of all 6,902 hashed web chunk files (gunzip, lowercase substring). Numbers are chunk-file counts. Short terms (react, d3, slate, moment, immer, hls, solid) include unrelated words; treat them as weak.
- @KD and @INFRA = keyword and infra counts from the earlier pass (method not preserved; weak signal).
- @MF = @R/AppxManifest.xml. @DJ = @R/locale/generated-touchdown/en-us/desktop.json. @NT = @R/NOTICE.txt. @CS = @R/CodeSignSummary files. @PE = REA result for ms-teams.exe (431 PE resources).

Coverage:
- Full static pass: package root listing, manifest, NOTICE, code-sign list, PE resource trees of ms-teams.exe and ic3rustcore.dll, web asset manifest, marker scan of all web chunks, en-us main locale keys.
- Partial: client-bindings JS (REA graph partial; see section 6).
- Not done: per-feature en-us locale chunks in full; native export tables and symbols; anything that needs running the app (not permitted by the audit rules).

## 1. Identity

- Product: Microsoft Teams, new client (MSIX). Package name MSTeams, version 26260.1701.5139.3736, x64, publisher CN=Microsoft Corporation [MF].
- ms-teams.exe version info: ProductVersion and FileVersion 26260.1701.5139.3736; FileDescription "Microsoft Teams"; CompanyName Microsoft Corporation (read with PowerShell VersionInfo, no execution) [SRC].
- Web client package: @msteams/maglev-included-web-assets 260813193.14.50801226 (described as the Maglev T2.1 client) [SRC @R/desktop-assets/package.json].
- Trusted UI package: @msteams/trusted-ui 260825087.0.50565228 (described as Maglev T2.x) [SRC @R/trusted-ui/package.json].
- Target OS: Windows.Desktop min 10.0.17763.0, max tested 10.0.26100.0 [MF].
- WebView2 external dependency: min 128.0.2739.67 [MF].
- Apps in the package: MSTeams = ms-teams.exe (FullTrust); MSTeams.Update = ms-teamsupdate.exe; msteamsautostarter = msteams_autostarter.exe (FullTrust); MSTeamsRemoteModuleContainer = packaged classic app in an AppContainer, partial trust [MF].
- Shipped config flags (names and values): "test/enableGCC": true; "tma/meetingAddinEnableVirtualAppointment": false [SRC @R/configuration_gcc.json]. @R/configuration.json is an empty object [SRC].
- Installer UX: AppInstaller flyout, accent #6264A7, button text "Teams", terms link to support.microsoft.com [SRC @R/Msix.AppInstaller.Data/MsixAppInstallerData.xml].
- Windows AI actions extension: com.microsoft.windows.ai.actions, id MSTeamsActions, registration action_manifest.json [MF, SRC @R/desktop-assets/action_manifest.json].
- ms-teams.exe PE resources: 431 total = 357 ICON, 71 GROUP_ICON, 1 MANIFEST, 1 VERSION, 1 named LIMITEDACCESSFEATURE "IDENTITY" (64 bytes). No RCDATA or STRING tables; strings sit in code or in resources.pri [REA @PE].
- ic3rustcore.dll: PE32+ x64, 516,288 bytes, one VERSION resource, no icons [REA].

## 1b. Tech stack

Native shell and media (SRC from directory listing, @CS, @NT and binary string tokens):
- C++ with MSVC vc143 runtime DLLs (msvcp140 family, vcruntime140 family, concrt140_app, vcomp140_app, vccorlib140_app, vcamp140_app).
- vcpkg-managed Boost 1.91 DLLs (atomic, filesystem, json, log, log_setup, thread, url).
- C++/WinRT (cppwinrt), WIL, Win2D (Microsoft.Graphics.Canvas.dll), Windows App SDK bootstrap (Microsoft.WindowsAppRuntime.Bootstrap.dll), WebView2 loader (WebView2Loader.dll) and Core WinMD.
- Rust: ic3rustcore.dll. Crate paths in the binary: dimpl (about 39 references), str0m-wincrypto, rand, once_cell, windows, nom, arrayvec, crypto 0.7.0, h2 (one reference). Source files referenced: handshake.rs, record.rs, extension.rs, use_srtp.rs, client.rs, server.rs, engine.rs, x25519.rs, digitally_signed.rs [SRC grep -a on the binary, no execution].
- Network and crypto: libcurl.dll; OpenSSL implied by dtls_v2_openssl.cpp inside skypert.dll (name only; not in the NOTICE list) [SRC].
- Storage: sqlite3.dll [SRC, NT].

Web client (Maglev, hosted in WebView2). All SCAN counts are chunk-file counts out of 6,902:
- UI framework: React DOM (react-dom in 10 chunks; the bare word react appears in nearly all chunks, so it is not evidence) [SCAN]. Fluent UI (fluentui in 153) and Griffel CSS-in-JS (griffel in 17) [SCAN]. Immer (ImmerES5Plugin chunk name) [AM].
- Data layer: GraphQL (graphql in 242), Apollo client (apollo in 155; chunk names chat-list-apollo, apollo-devtools-maglev-css, apollo-devtools-subscriber) [SCAN, AM]. Per-feature resolver and service modules: r_data-resolvers-* (190 chunk names) and services-* (170) [AM]. Redux (67), MobX (7), RxJS (13), Zod (197), lodash (17) [SCAN].
- Collaboration: Fluid Framework (FluidDocumentServiceFactoryContainer, FluidFramework-HashFallback, fluid-* chunks; OdspDriver, ConnectionManager and DeltaConnection namespaces) [AM, NS]. Yjs (yjs in 61 chunks) [SCAN].
- Real-time transport: Trouter (56), SignalR (110), STOMP (176), WebSocket (50), Substrate (298; chunk SignalServiceForSubstrate) [SCAN, AM].
- Auth and telemetry: MSAL (161), Sentry (236), Application Insights (36), OpenTelemetry (8); telemetry workers [SCAN, SRC].
- Web media: WebRTC (142), Opus (348), AV1 (91), VP9 (40), WebAssembly (17), DTLS (19), SRTP (17), insertable streams (4), SharedWorker (23) [SCAN]. Workers in @R/desktop-assets/workers (6 files): precompiled-web-worker (two builds), precompiled-webrtc-web-worker, precompiled-telemetry-web-worker, precompiled-editor-dedicated-index-readjuster-web-worker, precompiled-editor-shared-index-readjuster-web-worker. Worker entries in serve-configs add precompiled-shared-worker, precompiled-telemetry-shared-worker, precompiled-pluginless-encoded-streams (web and shared) and precompiled-wasmcv (web and shared; WASM computer vision) [SRC].
- Storage: IndexedDB (127), Service Worker (34 chunks; the desktop build ships service-worker-noop.js.gz instead) [SCAN, SRC].
- Editors and content: CKEditor 5 (ckeditor in 38), CodeMirror (23), Mermaid (100), Adaptive Cards (273) [SCAN, AM].
- Other libraries present by name: libphonenumber (8), ONNX (7), dayjs (1), moment (852; probably date handling, but substring noise, so INF) [SCAN].
- Boot page: @R/desktop-assets/unhashed-assets/index.html.gz decompresses to a page with nonce-templated scripts (ids loader-config, cache-chunks, loader) and meta tags containerVer and viewportContext. Decompressed scratch copy: @T/maglev_index.html [SRC].

Shipped JS bridges (@R/client-bindings, 9 files, 3.68 MB):
- UMD module names: @skype/skype-calling-electron (calling_electron_bundle.js), @skype/rt-js-bindings (rt_js_bindings_bundle.js), scpie-vdi-bridge, scpie-vdi-sc, scpie-vdi-webrr, slimcore-webview2 (scwv2_bundle.js), slimcorewebview2_trusted (scwv2_bundle_trusted.js). bundle.js and bundletrusted.js have no UMD name [SRC].
- Bridge factory: window.pieSharedBridgeFactory appears in bundle.js, bundletrusted.js, both scwv2 bundles and scpie_vdi_bridge_bundle.js [SRC grep]. Channel-style strings app/piebatch (7 uses) and app/webrr (2 uses) [SRC grep].
- REA JS graph: 9 parsed files, 752,153 AST nodes, zero Electron IPC operations recovered, partial coverage (section 6) [REA].

## 1c. Dependencies grouped by capability

Calling and real-time media (native):
- SlimCore: SlimCoreWebview2.dll (19.7 MB; video send, receive and render; DX11 shaders; C API file api.cc), skypert.dll (5.6 MB; Wi-Fi discovery, LLDP, geolocation access, DTLS with OpenSSL, WinRT helpers, RT persistence), SlimCV.dll (2.2 MB; computer vision), ssScreenVVS2.dll (screen share shaders), vdibridge_wv2.dll (VDI bridge, vendor detection, screen scraper interface) [SRC, @CS].
- RTM (real-time media): RtmMediaManager.dll (SDP parser and session model, ICE server, DNS cache, media platform, security code utils), RTMPLTFM.dll (22.8 MB; RTP and SRTP send and receive, RED redundancy, DTMF, RTP stats, echo detector, ADSP recording), RtmCodecs.dll (10.7 MB; SLIQ rate control and stats, NV12 shaders, capturer), RtmPal.dll (WASAPI devices, USB host controllers, HID context, Bluetooth info, VoIP call coordinator, virtual devices), RtmControl.dll (control injector) [SRC source-path tokens].
- ic3rustcore.dll: Rust DTLS, SRTP and WebRTC crypto core (section 1b) [SRC, REA].
- onnxruntime.dll (8.4 MB): on-device ML inference; use not traced [SRC, @CS].
- @skype/rt-js-bindings and @skype/skype-calling-electron: JS side of the RTM and calling stacks [SRC].

Cloud, auth, telemetry and data:
- Azure SDK for C++: azure-core-cpp, azure-storage-blobs-cpp, azure-storage-common-cpp (MIT) [NT; DLLs azure-core.dll, azure-storage-blobs.dll, azure-storage-common.dll].
- OneAuth (auth library) [NT]. Windows OAuth WinMD Microsoft.Security.Authentication.OAuth.winmd [SRC].
- mstelemetry (telemetry library) [NT]; Sentry, Application Insights and OpenTelemetry in web code [SCAN].
- odsp-lmtlib (OneDrive and SharePoint library component) [NT].
- outlooktma (Outlook meeting add-in, TMA) [NT]; MicrosoftTeamsMeetingAddinInstaller.msi (15.7 MB, Outlook meeting add-in installer) [SRC].
- minizip and zlib1.dll (archives and compression) [NT, SRC].
- folly (Meta C++ library) and gtest (GoogleTest) appear as NOTICE entries; gtest would be a test library, so its presence in a shipped build is INF [NT].
- cpp-async, vcpkg-boost, vcpkg-cmake-config, vcrtforwarders [NT].

UI, shell and Windows integration:
- WinRT metadata shipped (WinMD): Microsoft.Windows.AppLifecycle, ApplicationModel.Background (plus UniversalBGTask), ApplicationModel.DynamicDependency, ApplicationModel.WindowsAppRuntime, BadgeNotifications, Media.Capture, PushNotifications, Security.AccessControl, Storage, System.Power, System; Microsoft.Web.WebView2.Core; Microsoft.Security.Authentication.OAuth [SRC root listing].
- Win2D (Microsoft.Graphics.Canvas.dll), WebView2 (Microsoft.Web.WebView2.Core.winmd, WebView2Loader.dll), WIL, cppwinrt, Windows App SDK bootstrap [NT, SRC].
- Limited Access feature token IDENTITY in ms-teams.exe [REA @PE].

Database, search and other:
- sqlite3.dll (1.15 MB) and IndexedDB (web) [SRC, SCAN].
- ONNX Runtime (onnxruntime.dll) and ONNX in web chunks (7) [SRC, SCAN].
- libcurl.dll (753 KB), boost_json and boost_url DLLs (HTTP and parsing helpers) [SRC].

## 2. Directory and module map

### 2.1 Package root (@R, 95 entries; levels 1-2)
- ms-teams.exe (36.2 MB) - main full-trust shell: window management (window_placement_storage_*, window_cascade_impl, window_curtain_win, window_size_limiter_win, window_content_protection_manager_impl), toast cache, COM servers, Office integration, IOCP wait ops [SRC source-path tokens].
- ms-teamsupdate.exe (4.7 MB) - updater; also hosts the ECS COM server (argument -RegisterComServerForEcsTask) [SRC @MF].
- msteams_autostarter.exe (606 KB) - auto-start task TeamsTfwStartupTask [MF].
- ms-teams_ac.exe, ms-teamsupdate.exe, ms-teams_autostarter.exe - app execution aliases [MF].
- AppxManifest.xml (84 KB), AppxBlockMap.xml, AppxSignature.p7x, AppxMetadata/CodeIntegrity.cat, CodeSignSummary files (3 signing logs), NOTICE.txt (1,755 lines) [SRC].
- client-bindings/ - JS bridges (section 1b).
- desktop-assets/ - Maglev web client (7,033 files, 206 MB): unhashed-assets/index.html.gz and service-worker-noop.js.gz; hashed-assets/ (7,006 chunk files); workers/ (6 files: precompiled-web-worker in two builds, precompiled-webrtc-web-worker, precompiled-telemetry-web-worker, precompiled-editor-dedicated and shared index-readjuster web workers); serve-configs/ (ag08, ag09, bleu, delos, dod, gallatin, gcch, life, work; work and life each list 17 endpoint keys including google/oauthcallback, streaming-container.html, live-stream-container.html, serviceworker.js, unsupported-browser and worker entries); maglev-configs/ (ag08, ag09, dod, gallatin, gcch, life, work); action_manifest.json; asset-manifest.json; package.json [SRC]. work.json sets crossOriginIsolationVersion 4 and enablePwa true [SRC].
- trusted-ui/ - trusted UI web app (227 files, 2.26 MB): index.html, serve-configs/maglev.json, 224 named assets (section 2.3) [SRC].
- locale/ - generated-touchdown/ with 63 locale folders (af-za through zh-tw; en-us holds desktop.json only) and fallback-map.json (regional fallbacks such as en-au to en-gb) [SRC].
- presence/ - 26 tray and taskbar presence icons (available, away, busy, do-not-disturb, offline, OOF variants, unknown; dark and light), plus system_tray/ icons and CMakeLists.txt; 45 files in total [SRC].
- jumplist/ - 14 .ico files (calendar, new chat, new meeting, video; dark, light and high-contrast variants) [SRC].
- Audio/ - 24 .mp3 files: ringtones (ring, ringring, incoming-ringtone-level30 and level40, meetup_ring, screenshare_ring) and notification sounds (teams_notification, teams_meet_up_reminder, beep, bop, bounce, bubbles, bubblesloud, dripdrop, eureka, flutter, highscore, nextlevel, plink, remix, ripple, spacetime, wishgranted, wobble) [SRC].
- Images/ - 241 files: app tiles and logos (TeamsForWorkNewAppList at many sizes, TFLLogo.ico), avatars (AvatarBase, AvatarBaseDark, AvatarBaseLight) [SRC].
- x64/uc.tlb, arm64/uc-arm64.tlb, x86/ucx86.tlb - UC COM type libraries; GUID b9aa1f11-f480-4054-a84e-b5d9277e40a8 v1.0 is referenced by the manifest [SRC, MF].
- ecs_settings_editor.html (34.8 KB) - ECS Settings Editor page (compare rings, apply, restart, load descriptions file, search) [SRC].
- configuration.json (empty object) and configuration_gcc.json (two flags, section 1) [SRC].
- priconfig.xml, resources.pri and resources.scale-125, -150, -200, -400 .pri - MRT resource index; contains Teams_SendMessage_Description and Teams_ScheduleMeeting_Description [SRC].
- MicrosoftTeamsMeetingAddinInstaller.msi, Msix.AppInstaller.Data/ (AppInstaller UX), microsoft.system.package.metadata/ (empty) [SRC].
- Native DLLs (sizes): RTMPLTFM 22.8 MB, RtmCodecs 10.7, SlimCoreWebview2 19.7, skypert 5.6, onnxruntime 8.4, Microsoft.Graphics.Canvas 1.7, RtmMediaManager 2.1, RtmPal 1.8, SlimCV 2.2, sqlite3 1.2, azure-storage-blobs 3.1, azure-core 1.4, libcurl 0.75, ic3rustcore 0.52, ssScreenVVS2 0.53, vdibridge_wv2 0.74, RtmControl 0.18, resources.pri 1.2; plus boost, vc runtime, zlib, minizip, WebView2Loader and WinMD files [SRC].

### 2.2 Web client module families (desktop-assets, from the asset manifest names)
- Lazy app modules, async-apps-* (81): activity, add-cloud-storage-folder-dialog, ai-interpretation, annotation, attendee-view, audio-recap, biometric-enrollment, breakout-rooms, broadcast, broadcast-scheduling, calendar, callable-channel, calling, calls-main, campaign-notifications, captcha-challenge, cast, change-default, channel-pages, closed-captions, coach-report, collab-stage-view, configuration-agent, contacts, copilot, discover-surface, entity-more-info, esp, exploratory-license, facilitator-customization, facilitator-private-chat, file-picker, files, files-one-up-viewer, floating-ufd, flora-fre-dialog, gallery, headline-slices, help, hotdesking-notifications, inline-slices, lastmile-telemetry, meet, meeting-analytics, meeting-notes, meeting-options, meeting-recap, meeting-recap-chiclet, monetization, mtma, notifications, people, people-core, places, presence, presenter-chat, presenter-toolbar, public-preview, queues, saved-message-slice, search, sensitivity-update, settings, share, share-email, share-to-chat, shell-collaborative, skype-dial-pad, smb, store, survey-form, teams-and-channels, teams-gallery, teams-store-notifications, tfl-onboarding, transcript-content, unified-consent, upsell-platform, virtual-consumer-events, virtual-events, whiteboard [AM].
- apps-* (15): add-cloud-storage, ai-interpretation, broadcast-scheduling, channel-pages, files, flora-fre-dialog, headline-slices, hotdesking-notifications, monetization, platform-data.mock (mock data shipped), platform-service-enpoints, sensitivity-update, shell-collaborative, skype-dial-pad, upsell-platform-app [AM].
- calling-* (31), meeting-* (26), chat-* (11), copilot-* (10), call-* (6), video-* (6), components-* (56; calendar app host and modals, breakout rooms app, broadcast auto-rejoin screen, avatar), communities-* (44), discover-surface-* (8), platform-* (9; app side panel, deep link, MetaOS service, app rail, modal, chat with subscription), queues-* (3), appbar-control-* (8), compose-* (7), share-* (7; share tray, meeting invite dialog, camera setup), search-* (8; search-answer, search-answer-v2-override, search-app, contextual lazy chunks), nova-* (5; accounts settings renderer, auth end pages, mtma hub, meeting options; the meaning of nova is not confirmed), live-* (3), loop-* (2), whiteboard-* (3), fluid-* (6), presence-service (1), adaptivecard-* (3), topics-sdk-* (3), auth-* (3), config-* (8: ag, bleu, delos, dod, gallatin, gcchigh, life, prod), data-* (5), ams-video-player (2), streaming-* (2), closed-captions-* (2), coach-report-* (2), immersive-* (2), tfl-* (2), store-* (2), presenter-* (2), people-* (2), messaging-* (2), locked-teams-* (2), mail-icon-* (2) [AM].
- Opaque families (meaning not confirmed): sht-* (207, including Cortex topic-type strings), r_data-* (190 resolvers), services-* (170), scaht-* (100, search converged answer strings), categories-*-json (94, localized category names), webrec-* (57) and webrec2-* (60; webrec2-effects-backdrops suggests video effects), lazy-* (27), precompiled-* (20 workers) [AM, K].
- Large localized string bundles: InformationProtectionStrings0 to 85 (sensitivity label strings), VideoPlaybackStrings0 to 71, mcclocalizedstrings_*, dwsettingsdialogstrings_*, officeaihandoffpluginstrings_* [AM].

### 2.3 Trusted UI and embedded module names
- trusted-ui named assets (224): acc-settings, account-list, authendpages, auth-end-page-msa-tui-wrapper, cortex-topics-bootstrapper-topics-sdk, data-capture-pie-monitor, traffic-cop-pie-monitor, pie-stats-provider, diagnostic-data-viewer, libphonenumber-bundle, local-mru-files, main, me-control, mocked-native-modules, mtma-hub-page-view, mtma-missed-notifications, screen-share-tab, screenshot-selector, serviceBlocked, serviceBlockedDesktop, tests-mock-auth (test mock shipped), topics-sdk-aria-sdk, topics-sdk-km-api, topics-sdk-loki-api, unsupportedPlatform, plus per-locale *-trusted-ui-js bundles and fns0 to fns86 function groups [AM].
- Native source files embedded in binaries (file names only, found with grep -a; not executed):
  - ms-teams.exe: window_* managers, win_toast_cache, winregutils, windows_detection_module, window_controls_module, wm_message_observable, win_symbol_session.
  - skypert.dll: wifi_discovery, wifi_winrt_helpers, lldp, geolocation, geolocation_access, dtls_v2_openssl, dtls_key_cert_manager, app_capability, catalog_win, rt_persistent, platform.
  - RtmMediaManager.dll: MediaPlatformImpl, mmiceserver, mmdnscache, mmtask, mmdata, SDPParser, SDPSession, SDPMedia, SDPTable, RTPFormat, candidateinfo, security_code_utils, client_mp_message_handler, media_content_type.
  - RTMPLTFM.dll: EngineCreate, EchoDetectorWrapper, ADSP_Recording, rtpsend, rtprecv, rtpssrc, rtpsec, rtpred, rtpdtmf, rtpstats, rtpcreate, rtpevents, srtpprim, tcplistenerpipe.
  - RtmCodecs.dll: rtmcodecs, scCapturerFs, sliq_ratectrl, sliq_ratectrl_aq, sliq_stats, sliq_utils, session, bundle, tar.
  - RtmPal.dll: VirtualPlatform, VirtualDeviceWin, VirtualAudioSample, RtcPalVoipCallCoordinator, RtcPalUSBHostControllers, RtcPalHIDContext, RtcPalDeviceBluetoothInfoHelper, RtcPalDeviceAudioWASAPI, RtcPalEndpointVolumeController, RtcPalFilesourceAPI, Startup.
  - SlimCoreWebview2.dll: video_sender, video_receiver, video_player, video_host, video_device_manager_sc, video_device_enumerator, video_channel_listener, video_renderer_telemetry, api.cc, DX11 BGRA, YUV and YUV-BT709 shaders.
  - vdibridge_wv2.dll: vdibridge_wv2, CommProtocolImpl, CommProtocolHandler, DetectVendor, ScreenScraperInterface, TelemetryLogger, JsonObjectBuilder.
  - ic3rustcore.dll: see section 1b.
- Common header across most native DLLs: auf_init.hpp and AufInitializer (an internal framework; expansion not confirmed) [SRC].
- uc.tlb identifiers (type library strings): conversation, contact, conference, participant, endpoint, modality, sharing, channel, meeting, presence, status, notification, subscription, credential, search, device, history, access request, provider, collection, operation [SRC].

## 3. Feature candidates, grouped by the 12 taxonomy categories

Each bullet is: [tag] feature - pointer. Pointers: @K = en-us key prefix (count of keys with that first token unless marked substring); @NS = namespace (noisy); @AM = web chunk family from the asset manifest; @MF = manifest element; @DJ = desktop.json key; @SRC = file or path; @SCAN = chunk-file count from this run; DOC-F or DOC-S = public docs (section 9).

### 3.1 Messaging core
- [SRC] 1:1 chats, group chats and meeting chats - @K chat_* (519), chats_* (171; chats_messagePreview_chat_one_on_one, chats_messagePreview_chat_group_chat, chats_messagePreview_chat_meeting), bilateral_chat_dialog_* (21).
- [SRC] Chat list with filters, pins (with a pinned-chat limit), mute, read and unread - @K chatList_* (88), chat_list_channel_item_marked_as_read and _unread, chatList_max_pinned_chats_limit_reached_aria_label, chatList_chatMuted_aria_label; @AM chat-list-apollo.
- [SRC] Threads and reply chains - @K thread* (239, substring), reply* (121, substring), post_aria_last_reply_* (post_* 66), tmp_channel_reply_chain_moderation_disabled; @AM announcement-threaded-channel.
- [SRC] Reactions - @K reaction* (73, substring), notification_feeds_filter_menu_reaction_text, activity_feed_title_reaction_multi.
- [SRC] @mentions, including nearby-mention feed entries - @K mention* (151, substring), atMentions* (11), activity_feed_title_mention_nearby; @AM AtMentionLazy.
- [SRC] Rich text, markdown and hyperlinks - @K markdown* (10), insertHyperlink* (9); @AM CodeMirror, CKEditor 5 (ckeditor in 38 chunks), CodePageTopBar, AboutVideoRichTextEditor.
- [SRC] Compose box with add-ons and inline Copilot rewrite - @K compose_* (294); @AM compose-box-v2-renderer, compose-add-ons, compose-copilot-inline-rewrite-driver, compose-copilot-container, compose-message.
- [SRC] Edit, delete, copy, paste, drag and drop - @K edit* (24), delete* (34), copy* (18), paste* (28), drag* (14), drop* (13); @AM DragDropDark, DragDropLight.
- [SRC] Drafts - @K draft* (51, substring). [DOC-S] A Drafts area for unsent messages was added in 2026.
- [INF] Scheduled send - not confirmed: no send_later key (0 hits); scheduled* keys (31) are meeting-related.
- [SRC] Pins, saved messages and bookmarks - @K pin* (232, substring; collaborative_tabs_coachmark_bullet_pinned_messages), saved* (48, substring), bookmark* (2); @AM saved-message-slice (async-apps-saved-message-slice).
- [SRC] Forwarding, including move messages - @K forward_* (51), move_messages_confirm_* (69). [DOC-S] Up to five messages can be passed on at once, order kept (2026 roundups).
- [INF] Polls - weak: poll* keys (7); no poll namespace found.
- [SRC] Audio clips and video messages - @K compose_audio_clip, compose_video_clip, clip* (20); @NS videoMessage (38), videoCapture (78; capture page record and screen share); @AM video-message, video-attachment-light.
- [SRC] Read state and typing - @K read* (588, substring), unread* (58, substring); typing (1 key; weak).
- [SRC] Presence and status - @K presence_* (52 prefix; presence_available, presence_away, presence_berightback), status_* (165 prefix; status_update_*); @NS presence (57), contacts (167; contact_status_*); @DJ statusbaritem keys.
- [SRC] Profile cards and persona editing - @NS livePersonaCard (51; lpc_* keys), livePersonaEditor (146; lpe_* keys: add name, add phone, add address, add contact).
- [SRC] Translation of messages and transcripts - @K translat* (76, substring), translation_* (9).
- [SRC] Emoji, GIFs, stickers and Giphy - @K emoji (59, substring), gif (28), giphy (15), sticker (13).
- [SRC] Announcements - @K announcement* (28); @AM announcement-dialog-in-meeting, announcement-threaded-channel.
- [SRC] Sensitivity labels on content - @K sensitivity* (55 prefix); @AM InformationProtectionStrings0 to 85.
- [SRC] Email to a channel - @K channel_email_setting_domains_input_limit_error.
- [SRC] Moderated channels and posting limits - @K messages_onlyChannelModeratorsCanPost, tmp_channel_post_settings_replies_limited, tmp_channel_meet_now_disabled_moderated_tooltip.
- [SRC] Loop components inline in chat - @K message_syncing_Loop_component_banner_message; @AM LoopFileChiclet, loop-preview-app.
- [SRC] Adaptive Cards in messages - @K adaptive_card* (33); @AM adaptivecard, adaptivecard-react, adaptivecard-diagnostics (adaptivecard in 273 chunks).
- [SRC] Message extensions (compose and search) - @K message_extension* (34).
- [SRC] Bot citations and message controls - @K message_bot_controls_citations_label_mf, message_bot_controls_senstivity_message_author (spelling as in source), bot_citation_loading_citation.
- [SRC] Slash commands in chat - @K slash_command_* (108 prefix), autosuggest_chat_command_action.
- [SRC] Calls and meetings started from chat - @AM chat-call-button, chat-audio-drop-in-start-button, chat-audio-drop-in-live-roster-button, chat-join-screen-share-button, chat-reject-call-button.
- [SRC] Ask Copilot hand-off prompts - @K ask_copilot_handoff_prompt* (9).
- [SRC] Cross-post trust indicator in shared channels - @K cross_post_trust_indicator_text_mf, cross_post_trust_indicator_resolving, channel_card_trust_indicator.
- [SRC] Activity-feed message previews - @K activity_feed_message_preview_* (forwarded message, meeting, unnamed root thread, replied with quote).
- [INF] Praise and recognition - no praise keys (0 hits). The recognition* keys (55) are face-profile strings, not praise.

### 3.2 Org and membership
- [SRC] Standard, private and shared channels; channel types - @K channel_* (211 prefix), private* (47, substring), shared* (166, substring); @AM channel-pages, channel-notification-settings, channel-dashboard, channel-header-wrapper.
- [SRC] Team management - @K teams_* (73 prefix); @AM teams-and-channels-app, teams-dashboard, teams-grid, teams-compose-box, teams-post-compose-box.
- [SRC] Shared-channel audience awareness - @K tmp_channel_shared_channel_audience_awareness_banner_* (including a post-compose trust indicator).
- [SRC] External users in channels and chats - @K peoplePicker_channel_external_users_suffix_singular and _plural, external* (90, substring).
- [SRC] Guest users and invite banners - @K guest* (46, substring), tfw_invite_guest_users_banner_sending_text.
- [SRC] Cross-tenant sign-in and B2B redemption - @K auth_tenant_redemption_click_text, auth_tenant_picker_header, auth_home_tenant_placeholder, auth_tenant_list_aria_label (tenant* 86, substring).
- [SRC] Multiple accounts - @K me_control_add_another_account*, auth_account_* (42, prefix auth_), auth_account_transfer_active_call_button; @AM account-list, acc-settings, auth-account-list-meeting-join, me-control (trusted UI).
- [SRC] Roles and permissions - @K permission* (527, substring), compose_change_permissions, available_apps_grid_column_header_permissions; @NS peopleTargeting (298).
- [SRC] Admin controls - @K admin* (68, substring), policy* (89, substring); @NS guardians (admin_setting_disabled_tooltip_text).
- [SRC] Management of teams, channels and apps - @K manage_* (210 prefix), manage_apps_agent, manage_apps_bot_compliance_*, manage_app_threads_loading_announcement.
- [SRC] Sidebar organization and reordering - @K simple_collab_left_rail_single_point_reordering_setting_label (and _message, _sub_message_one), simple_collab_switcher_off; @AM SimpleCollabBulkMoveToSectionMenuItem; @NS simpleCollab (noisy count 1,043).
- [SRC] Communities (membership with approval, admin checklist, block members, create wizard, join and approve, report concern) - @K communities_* (630 prefix; communities_create_require_approval_button, communities_join_approve_confirmation_dialog_content, communities_report_concern_menu_title); @NS communities (685 keys); @AM communities-* (44 chunks; communities-admin-checklist, communities-block-members, communities-create-dialog, communities-create-channel, communities-apply-to-feature-dialog).
- [SRC] Teams for Work (TFW) - @K tfw_* (122 prefix); @MF startup task TeamsTfwStartupTask.
- [SRC] Teams Free for Life (consumer) - @K tfl_* (178 prefix), tflFre_* (79); @NS tflOnboarding (160), tflHomepage (19); @AM tfl-onboarding-app, tfl-calendar-people-picker, async-apps-tfl-onboarding; @SRC serve-configs life.json.
- [SRC] Education - @NS edu (60: classwork and assignment icons, active meeting, change team picture); @K class* (14), assignment* (15).
- [SRC] Guardians (parent accounts) - @NS guardians (147: add_guardian_data_button, add_new_parent_button_tooltip_text).
- [SRC] SMB - @NS smb (143); @AM async-apps-smb; @K smb_* (17).
- [SRC] Teams Labs and Teams Insider preview programs - @NS teamsLabs (78; all_messages_with_badging_feature_name, accessible_message_replies_for_screen_readers_feature_name), teamsInsider (94).
- [SRC] Exploratory (trial) license states - @K exploratory_license_error_page_* (45); @AM exploratory-license.
- [SRC] Local cache of policies and settings - @NS UpdateLocalCacheSupportedPoliciesAndSettingsNamespaces (1,794 keys; the name indicates cached policy and settings namespaces).
- [SRC] Invite and join links - @K tfw_chats_invite_join_link_info.
- [SRC] Admin campaigns about agent features - @K campaign_notification_admin_agent_feature_* (110 prefix campaign_), admin_agent_copilot_campaign_notification_title.
- [SRC] Contact management - @NS jana (130; jana_add_contact_button, jana_add_to_contacts_action_label; codename, INF: contacts management); @K contacts_action_* (164 prefix; contacts_action_block, contacts_action_call, contacts_action_chat, contacts_action_copy); @AM contacts, async-apps-contacts.
- [DOC-F] Shared channels: owners create and manage them, and visibility follows membership; a channel cannot be switched between standard, private and shared after creation (Learn shared-channels pages; DOC-S details).
- [DOC-S] Guest accounts in the home tenant cannot join shared channels directly; outside users join through B2B direct connect (support articles; secondary summaries disagree on details).

### 3.3 Calls and meetings
- [SRC] Audio and video calls - @K calling_* (185 prefix), calling_call, calling_video; @NS calling (4,986; noisy); @AM calling-* (31 chunks), calling-app, calling-app-post-critical, calling-cdl, calling-byod, calling-broadcast-container, calls-main, calling-alert-dialog.
- [SRC] Call health, call blocking, call transcript panel - @AM call-health-panel, call-health-service, call-blocking-service, call-transcript-panel, call-screen-image.
- [SRC] Call hold, transfer and lobby - @K hold* (279, substring), transfer* (6), lobby* (23, substring).
- [SRC] Callable channels (calls to a channel) - @AM callable-channel.
- [SRC] PSTN dial pad and DTMF keypad - @AM skype-dial-pad, lazy-calling-dtmf-keypad; @K pstn* (2).
- [SRC] Huddles (ad-hoc spaces) - @K huddle* (3), huddle_conv_ux_channel_thread_coachmark_img_alt, devices_store_space_huddle_space.
- [SRC] Voicemail and call forwarding - @K voicemail* (4), configuration_agent_actiontype_forwardallcallstovoicemail.
- [SRC] SMS and Phone Link messaging - @K phonelink_sms_campaign_title, phonelink_sms_campaign_description, chat_list_is_sms_text, autosuggest_sms_external_tag; @K sms* (79, substring).
- [SRC] Contact center queues and agent monitor - @NS queues (634; agent_monitor_coachmark_content, aria_label_bulk_opt_agent_checkbox); @AM queues-app, queues-app-bulk-opt-dialog, queues-app-recap; @K call_queue_manage_users_and_groups_subpage_title.
- [SRC] Voice admin settings - @NS voiceAdminSettings (176).
- [SRC] Emergency calling and location - @K emergency_section_subpage_title, messages_emergency_sms_not_supported_title, messages_emergency_sms_not_supported_body; @K configuration_agent_actiontype_locationforemergencycalls (in @NS configurationAgent); @AM call-emergency-location-loader; @SRC skypert.dll geolocation and geolocation_access sources.
- [SRC] Screen sharing (share tray, picker, content sharing) - @AM share-tray, share-tray-content-camera-setup-dialog, screen-share-tab and screenshot-selector (trusted UI); @K calling_contentsharing_error_message_file_format_not_supported, shortcut_accept_incoming_screenshare_only_call; @MF graphicsCapture capabilities.
- [SRC] Meetings (scheduled, Meet Now, options) - @K meeting_* (148 prefix); @NS meetingOptions (717), meetingFlyout (381), meetingRoom (217), meetingMeetApp (131), meetingSettings (10), meetingBot (111); @AM meeting-options, meeting-controls-panel, meeting-lock-back-image, meeting-details-editor-area-v, meeting-apps-flyout-renderer, meeting-attendance-teams-app-wrapper.
- [SRC] Meeting agent settings - @AM meeting-agent-settings-container.
- [SRC] Meeting-room and meet-for-work devices - @NS meetForWork (564; ultrasoundSpeaker, digitalSignageSettings, faceRecognizedUserScores, teamsConfiguration).
- [SRC] Breakout rooms - @NS breakoutRooms (471); @AM async-apps-breakout-rooms, components-breakout-rooms-app.
- [SRC] Webinars, town halls, broadcasts and virtual events - @NS webinar (368), broadcast (286), broadcastScheduling (308), virtualEvents (3,050), virtualConsumerEvents (752), audienceViewDialog (109); @AM async-apps-broadcast, async-apps-broadcast-scheduling, async-apps-virtual-events, async-apps-virtual-consumer-events, broadcast-scheduling, streaming-end-screen, audience-view-dialog-locale-*.
- [SRC] Immersive events - @NS immersiveContent (454); @AM immersive-event-end-screen, immersive-reader-service.
- [SRC] Presenter and attendee experience (presenter chat and toolbar, attendee view, collaborative stage) - @AM presenter-chat-app, presenter-toolbar-app, attendee-view-app, collab-stage-view, async-apps-presenter-chat, async-apps-presenter-toolbar, async-apps-attendee-view, async-apps-collab-stage-view.
- [SRC] Facilitator and facilitator private chat - @AM async-apps-facilitator-customization, async-apps-facilitator-private-chat.
- [SRC] Live and closed captions, AI interpretation (live interpreter) - @K caption* (29), live* (146, substring); @AM closed-captions-panel, closed-captions-settings-panel, live-interpretation-join-dialog-container, live-interpreter-promotion-dialog-container, streaming-interpreter-settings-dialog-container, async-apps-closed-captions, async-apps-ai-interpretation.
- [SRC] Translated captions and transcripts - @K translat* (76, substring); @AM async-apps-transcript-content.
- [SRC] Recording and transcription - @K recording* (170, substring), transcript* (55, substring); @NS meetingSettings (save_meeting_recordings_title, save_meeting_recordings_description).
- [SRC] Meeting recap and audio recap - @NS meetingRecap (704); @AM meeting-recap, meeting-recap-chiclet, audio-recap, copilot-recap, async-apps-meeting-recap, async-apps-audio-recap, services-MeetingRecapSwaservice. [DOC-F] Recap shows AI notes, suggested tasks and highlights, and needs Teams Premium or a Copilot license; audio recap requires Copilot.
- [SRC] Meeting notes and meeting brief - @AM meeting-notes, meeting-notes-app, MeetingNotesComponent, MeetingNotesShareService, MeetingBrief, async-apps-meeting-notes.
- [SRC] Meeting analytics and AI insights - @NS meetingAnalytics (279; ai_insights_add_task_title, ai_insights_assignee); @AM meeting-analytics-app, async-apps-meeting-analytics.
- [SRC] Speaker coach and coach report - @NS coachReport (271; coach_report_analytics_speaking_time); @AM coach-report-app, coach-report-teams-app-wrapper, async-apps-coach-report.
- [SRC] Meeting collaboration (agenda and agenda timer) - @NS meetingCollaboration (679; agenda_timer_start_button_text, agenda_timer_stop_button_text).
- [SRC] Attendance report and meeting artifacts - @NS artifactsPlatform (46; attendance_report_available, content_bin_default_filename, carousel_title_meeting_details).
- [SRC] Virtual appointments - @SRC @R/configuration_gcc.json flag tma/meetingAddinEnableVirtualAppointment (false). [DOC-S] Virtual appointments include a branded lobby reachable by link.
- [SRC] Meeting backgrounds, video effects and avatars - @NS decorateMeetingBackground (38); @AM video-effects-and-avatar-panel, video-effects-and-settings-panel, video-effects-more-view-panel, webrec2-effects-backdrops.
- [SRC] Face recognition and biometric enrollment - @NS biometricEnrollment (67); @AM biometric-enrollment; @K face* (25, substring), recognition_* (55, face-profile and biometric export strings).
- [SRC] Host controls and NDI streaming - @NS hostControls (77; ndi_auto_install_information, ndi_auto_error_download_link, isolated_audio).
- [SRC] Cast to room devices - @NS cast (13; cast_title, cast_button, do_not_join_meeting_text); @AM async-apps-cast; @SRC cast endpoint live-stream-container.html.
- [SRC] Hololens and Mesh indicators - @NS hololens (20; indicator_hololens_long, indicator_meshapp_long).
- [SRC] Post-meeting and post-call survey - @NS survey (320); @K survey_notification_setting_* (3); @AM async-apps-survey-form.
- [SRC] Recurrence for meetings - @NS recurrence (49).
- [DOC-S] Premium meeting features: RTMP-In for webinars, green rooms, registration approval, waiting lists; live translated captions listed as Premium in one roundup.

### 3.4 Search and discovery
- [SRC] Global, scoped and filtered search - @K search_* (18 prefix; 325 substring), unifiedSearch (32 keys); @AM search-app, search-lazy-chunk, search-contextual-lazy-chunk, search-contextual-meeting-result-item, teams-search-prefetch-configs, data-resolvers-search.
- [SRC] Copilot and AI answers in search - @AM search-answer, search-answer-v2-override, scaht-search-answer-strings, scaht-searchconvergedanswerteamsstrings-* (100 locale chunks); @K copilot_* (424, substring).
- [SRC] Autosuggest and recent items - @K autosuggest_* (335 prefix; autosuggest_apps_group_heading, autosuggest_channels_group_heading, autosuggest_chat_command_action), recent_* (29); @NS autosuggest (752).
- [SRC] People directory and people picker - @K people_* (17 prefix), person_* (30 prefix), peoplePicker_* (53 prefix); @NS people (353), peoplePicker (145), peopleMsa (732); @AM people-app-main-view, people-app-left-nav, people-core, async-apps-people, async-apps-people-core.
- [SRC] Files search and file preview - @K files_* (991 prefix), file_* (28 prefix), file_preview (61, substring); @AM files-one-up-viewer, async-apps-files-one-up-viewer, async-apps-files; @NS lightbox (26).
- [SRC] Discover feed (sorted by recency or relevance; block post) - @K discover_feed_recency_sorted, discover_feed_relevance_sorted, discover_surface_block_post*; @NS discoverSurface (58); @AM discover-surface-app, discover-surface-caught-main, discover-surface-empty-main, discover-surface-error-main, discover-surface-l2-back, discover-surface-up-dialog, async-apps-discover-surface.
- [SRC] Headline and inline slices (feed components) - @AM apps-headline-slices, async-apps-headline-slices, async-apps-inline-slices.
- [SRC] Keyboard navigation help - @K keyboard_navigation_help_files_title, keyboard_navigation_help_activity_feed_title, keyboard_navigation_help_app_bar_title.
- [SRC] Topics and knowledge (Cortex topics SDK, km and loki APIs) - @AM topics-sdk-km-api, topics-sdk-loki-api, topics-sdk-aria-sdk, cortex-topics-bootstrapper-topics-sdk; @AM sht-cortextopictypeswebstrings_* (locale strings). [INF] Cortex is the internal name of the Project Cortex line (later Viva Topics); medium confidence.
- [SRC] Galleries and media search - @NS gallery (24), lightbox (26); @AM async-apps-gallery, teams-gallery-app, async-apps-teams-gallery.
- [SRC] Contextual search in meetings - @AM search-contextual-meeting-result-item.
- [SRC] Immersive reader - @AM immersive-reader-service.
- [SRC] Yammer and Viva Engage strings present - @SCAN yammer (389 chunks; weak).

### 3.5 Notifications and attention
- [SRC] Activity feed and feed filters - @K activity_* (147 prefix), notification_feeds_filter_menu_* (mentions, missedcall, reaction, replies, voicemail), feed_selected_voicemail_filter_tooltip; @NS activity (384); @AM activity-app, activity-main-pane, activity-feed-dialogs, activity-transport, async-apps-activity.
- [SRC] Missed notifications and missed calls - @NS mtmaMissedNotifications (17); @AM mtma-missed-notifications (trusted UI), async-apps-mtma, async-apps-notifications.
- [SRC] Toast notifications, in-app and OS - @K toast_* (62 prefix), toast_browser_notification_button, toast_custom_calling_urgent, toast_custom_dismiss_button; @NS toastNotifications (165); @MF windows.toastNotificationActivation (COM class 816EB0E0-F985-4919-BDA3-315878A4ED80).
- [SRC] Taskbar badges - @K badge* (54, substring); @SRC Microsoft.Windows.BadgeNotifications.winmd.
- [SRC] Do not disturb, out of office and busy states - @K dnd_* (26 prefix), do_not_disturb* (8), oof* (22, substring); @NS presence (dnd_info_banner_message).
- [SRC] Mute and follow controls - @K chatList_chatMuted_aria_label, follow* (70, substring); @AM channel-follow-notification-settings-entity-view, channel-notification-settings.
- [SRC] Urgent and priority calling notifications - @K toast_custom_calling_urgent; @NS configurationAgent (configuration_agent_actiontype_allownotificationfromurgentandprioritycontacts).
- [SRC] Notification sounds - @SRC Audio/ (24 files); @NS configurationAgent (configuration_agent_actiontype_useteamssoundfornotificationoverosalertsounds).
- [SRC] Reminders - @K reminder* (82, substring), reminders_* (12 prefix).
- [SRC] Campaigns and what's-new messages - @NS campaignNotifications (165), whatsNew (467); @K tfl_whats_new_synthetic_notification_title, campaign_notification* (110 prefix). [DOC-S] Monthly what's-new roundups exist for 2026.
- [SRC] Notification settings per channel and huddle notifications - @K notification_settings_huddle_start_notifications, notification_* (168 prefix); @AM channel-notification-settings.
- [SRC] Push and background wake-up (native) - @SRC Microsoft.Windows.PushNotifications.winmd, Microsoft.Windows.ApplicationModel.Background.winmd, msteams_autostarter.exe.
- [SRC] Feed "For you" and "Now" cards - @K flyout_for_you_static_now_card_* (for example create community), flyout_* (188 prefix); @AM headline-slices.
- [SRC] Survey notification setting - @K survey_notification_setting_title, survey_notification_setting_label, survey_notification_setting_description.
- [INF] Digest mode - digest key count 1; not confirmed.
- [INF] Quiet hours and working hours as named features - quiet_hours (0 keys) and working_hours (0 keys); "working hours" appears only in the earlier keyword counts (@KD, 61).

### 3.6 Apps and extensibility
- [SRC] Teams app store and catalog - @K store_* (147 prefix; store_agent_uses_ai, store_app_tile_first_rating_text, store_app_tile_lob_text), extensioncatalog_* (97 prefix; extensioncatalog_appAssignmentsName), ext_store_workflows_title; @AM store-app, store-fre-modal, async-apps-store, async-apps-teams-store-notifications; devices_store_* keys (devices store banners).
- [SRC] Extensibility dialogs, consent and app picker - @K extensibility_* (520 prefix; extensibility_addAppDialogAppCardPermission1, extensibility_addedOneWayBotToTeamOrChat, extensibility_app_picker_create_project_agent_create_button), app_install_sso_* (sso keys 29, substring), share_app_disabled_consent_dialog_*; @AM lazy-extensible-app-with-consent-container.
- [SRC] Platform apps: side panel, deep links, app rail, subscriptions - @AM platform-app, platform-app-deeplink-to-sidepanel-execute, platform-app-rail, platform-app-service, platform-app-details-renderer, platform-app-metaos-service, platform-conversation-pane-container, platform-chat-with-subscription, platform-modal; @K platform_3p_* (third-party cookie dead-end and open-in-browser strings).
- [SRC] Tabs in channels and chats - @K tab* (681, substring), tab_access_denied_header, tab_access_denied_body, tab_activity, tabs_* (17 prefix); collaborative_tabs_* (38).
- [SRC] Personal apps and app pins - @K app_* (790 prefix), personal* (19 prefix); @AM apps-add-cloud-storage, apps-files, apps-channel-pages, apps-shell-collaborative, apps-upsell-platform-app.
- [SRC] Bots and bot cards - @K bot_* (72 prefix), botcards_* (98 prefix; botcards_action_blocked_reason_botblockedbyadmin, botcards_action_blocked_reason_botblockedbyuser, botcards_action_blocked_reason_connectorsender), bot_cards_mos3_agent_permission_banner, bot_citation_message_unavailable.
- [SRC] Slash commands - @K slash_command_* (108 prefix; slash_command_files_description, slash_command_default_file_placeholder).
- [SRC] Message extensions and Adaptive Cards - @K message_extension* (34), adaptive_card* (33); @AM adaptivecard family (3 chunk names; 273 chunk hits).
- [SRC] Connectors and workflows (Power Automate) - @K connector* (51, substring), workflows_* (15 prefix), ext_workflows_coachmark_title, extensibility_workflows_approval, extensioncatalog_appFlow_tabApprovalsName; [DOC-S] Office 365 Connectors were retired from Teams in May 2026; Workflows webhooks are the replacement.
- [SRC] Input extensions (app-based link previews) - @K inputExtension_* (93 prefix; inputExtension_appBasedLink_expand_button_aria_label), inputExtension_action_tooltip.
- [SRC] Outlook and Office integration - @MF COM class Office Integration 88435F68-FFC1-445F-8EDF-EF78B84BA1C7; @NT outlooktma; @AM officeaihandoffpluginstrings_* (locale strings), mtma-hub-page-view (trusted UI); @K ask_copilot_handoff_prompt*.
- [SRC] Share to Teams from other apps - @NS shareToTeams (185; copy_link_success_text, account_mismatch); @MF windows.shareTarget "Send to Microsoft Teams" (any file type; storage items, text, bitmap); @AM share-form, share-form-email-entry, share-to-chat, share-email.
- [SRC] Chat embedded in other apps - @NS embed (33; embed_chat, embed_add_people, embed_pop_out_chat, embed_chat_policy_error_aria_announcement).
- [SRC] Custom Copilot and Copilot-enabled apps - @NS customCopilot (248; termsOfUseUrl, privacyUrl), copilotChatApp (36); @K contextless_config_app_requires_copilot, contextless_config_app_requires_copilot_install, contextless_config_app_not_available.
- [SRC] App management and approvals - @K manage_apps_agent, manage_apps_bot_compliance_cancel_button_title, manage_app_threads_loading_announcement, app_card_error_admin_approval_required, app_device_permission_settings_dialog_*.
- [SRC] Windows AI actions (Chat and Schedule) - @SRC @R/desktop-assets/action_manifest.json: MSTeams.Action.Chat (action-sip URI with the contact e-mail) and MSTeams.Action.Schedule (action-schedule URI); both have isAvailable set to false in the shipped file, and usesGenerativeAI is false for both.
- [SRC] Developer and test artifacts shipped in the web bundle - @AM tests-mock-auth, mocked-native-modules, platform-data.mock; @SRC @R/ecs_settings_editor.html.
- [DOC-F] Developer platform for agents and apps: Teams SDK, Microsoft 365 Agents SDK, Microsoft 365 Agents Toolkit (section 4).

### 3.7 Agents in the room
- [SRC] Agents as channel members - @K channel_agents_section_title, channel_agents_list_aria_label, odsl_add_agent_to_conversations (and _tooltip variants); @AM channel-pages; [DOC-S] Channel Agent is a per-channel agent (public preview), created by default for new channels and shown in the channel app list and roster.
- [SRC] Agent details and permissions - @K agent_details_access_instructions_title, agent_details_additional_information, agent_details_agent_capability, agent_details_ai_disclaimer, agent_details_permissions_perform_actions, agent_details_stats_platform_label, agent_details_stats_platform_microsoft; @NS meetingCollaboration (agent_app_turn_off_button, agent_app_profile_card_gcp_action_one_description).
- [SRC] Agent @-invocation - @K project_agent_at_mention_button_label, project_agent_accordion_item, atMentions* (11).
- [SRC] Project agents with knowledge resources (chat or meeting scope) - @K project_agent_configuration__knowledge_resources_options_view_more_title_chat and _meeting; app_profile_card_project_agent_learn_more; ext_actionable_app_profile_card_project_agent_consent; about 151 project_agent keys in total.
- [SRC] Agent skills and preferences - @NS nova (307; agent_preferences_and_skills_create_in_progress_toast, agent_preferences_and_skills_create_success_toast, agent_skill_create_failure_toast, agent_skill_create_in_progress_toast); @AM nova-components-accounts-settings-renderer, nova-components-auth-end-pages, nova-components-mtma-hub, nova-fluent-nova-with-mtma, nova-meeting-options (the nova name is not expanded; INF that it covers agent skills).
- [SRC] Configuration agent (natural-language settings actions) - @NS configurationAgent (180; configuration_agent_actiontype_activatemeetingroommode, _adjustbrightness, _allownotificationfromurgentandprioritycontacts, _appbarwhenresizing, _forwardallcallstovoicemail, _locationforemergencycalls, _atmentionsnearbyinchatconfiguration, _useteamssoundfornotificationoverosalertsounds); @AM async-apps-configuration-agent.
- [SRC] Agent status banners and permission prompts - @K bot_cards_mos3_agent_permission_banner, app_profile_card_project_agent_learn_more.
- [SRC] Agent admin campaigns - @K campaign_notification_admin_agent_feature_team_setup_title, campaign_notifications_admin_agent_feature_guided_help_title, campaign_notifications_admin_agent_feature_service_access.
- [SRC] Agent monitor in contact-center queues - @K agent_monitor_coachmark_content, agent_monitor_coachmark_title, aria_label_bulk_opt_agent_checkbox; @AM queues-app.
- [SRC] Agent panes and AI control panels - @AM agent-assist-pane, agents-container-panel, ai-controls-panel, meeting-agent-settings-container.
- [SRC] Copilot inside chats and channels - @K copilot_* (424, substring), copilot_smart_response_button_title, copilot_refresh_license_banner, tmp_channel_copilot_button_open_label, tmp_channel_copilot_header_label; @AM copilot-container, copilot-summary-card-container, copilot-reply-summary-card-container, copilot-plugin-manager, copilot-right-panel-more-menu-button, copilot-synthesis, copilot-only-mode-ufd-control, copilot-card-renderer.
- [SRC] Copilot limits on group-chat files - @K group_copilot_multiple_files_warning_mf, group_copilot_unsupported_file_types_warning_mf, group_copilot_multiple_files_and_unsupported_file_types_warning_mf.
- [SRC] Human hand-off to Copilot or Office - @K ask_copilot_handoff_prompt, ask_copilot_handoff_prompt_a, ask_copilot_handoff_prompt_acronym, ask_copilot_handoff_prompt_c; @AM officeaihandoffpluginstrings_* (locale strings).
- [SRC] Bot and agent message controls - @K message_bot_controls_citations_label_mf, bot_citation_loading_citation, bot_citation_message_unavailable_excerpt.
- [SRC] Teams chats and meetings as agent knowledge - [DOC-S] Copilot Studio can take Teams channel, meeting and group-chat messages as knowledge sources; each asker sees only chats they can open.
- [SRC] Approval flows around app actions (human in the loop) - @K approval* (7 keys: app_card_error_admin_approval_required, extensibility_workflows_approval, extensioncatalog_appFlow_tabApprovalsName, communities_create_require_approval_button). [INF] No agent-specific approval flow keys found.
- [INF] Agent PC, browser and sandbox sessions - sandbox (0 keys), computer_use (0 keys); no agent-browser or sandbox strings. Not present in the client, as far as these strings show.
- [INF] Scheduled and cron agent work - routine (0 keys) and cron (1 non-agent key); not confirmed.
- [INF] Agent memory per person or room - memory* keys are diagnostics strings (brb_memory_snapshot_consent_dialog_hint_text_with_dpn_link, brb_feedback_uploading_in_progress_memory), not agent memory.
- [SRC] In-app feedback and problem-report flow (brb-prefixed keys; expansion not confirmed) - @K brb_category_* (223 prefix), brb_feedback_*, brb_memory_snapshot_consent_dialog_*. [INF] Looks like the feedback tool with a memory-snapshot consent step.
- [SRC] Agent cost and quota - @K limit* (53, substring) are UI limits (replies, pinned chats); @NS storageQuota (8; OneDrive storage banners). No agent cost or token-quota keys found.
- [INF] Multi-agent coordination - no direct key; the nearest are project agent and channel agent strings.

### 3.8 Collaborative artifacts
- [SRC] Loop components and Loop pages - @K loop_* (49, substring), shortcut_add_loop_paragraph, message_syncing_Loop_component_banner_message; @AM loop-pre-warm, loop-preview-app, LoopFileChiclet; @SCAN loop.office (116 chunks in the earlier pass, @INFRA).
- [SRC] Whiteboard - @K whiteboard_* (4 keys); @AM whiteboard-chat-tab, whiteboard-modal-dialog, whiteboard-renderer, async-apps-whiteboard; @SCAN whiteboard (615 chunks).
- [SRC] Planner, tasks and lists - @K planner* (7), task* (60, substring), channel_lists_add_new_list, channel_lists_link_existing_list, channel_lists_tab_default_name; @SCAN planner (312 chunks).
- [SRC] Wiki tab - @K tab_wikiApp_allWikis_name, tab_wikiApp_myWiki_name, tab_wikiApp_recentWikis_name, wiki_page_as_file_export_note.
- [SRC] Canvas tab - @K extensioncatalog_tabCanvasName, extensioncatalog_tabCanvasDescription, extensioncatalog_tabCanvasDescriptionLong.
- [SRC] Forms and surveys - @K form_* (182, substring); @AM async-apps-survey-form.
- [SRC] Collaborative tabs and simple collab (channel sections) - @K collaborative_tabs_* (38), simple_collab_* (472 prefix); @AM SimpleCollabBulkMoveToSectionMenuItem.
- [SRC] Files and cloud storage - @K files_* (991), onedrive* (44, substring), sharepoint* (21, substring); @NS storageQuota (8; od_banner_* keys); @AM add-cloud-storage-folder-dialog, file-picker, async-apps-file-picker, P2PFilesListContainer, P2PFilesListEmptyStateView, P2PSharedListContainer, P2PSharedListEmptyStateView, P2PSharedListWorkingsetEmptyStateView, SharepointShareService, SharepointNotificationService, AudioFilesUpload, DeleteConfirmationDialog.
- [SRC] Content bin and shared meeting content - @NS artifactsPlatform (46; content_bin_missing_access_aria_label, carousel_title_meeting_details).
- [SRC] Comments and rich comments - @NS commentStream (34); @AM RichCommentsModule.
- [SRC] Code blocks and code pages - @AM CodePageTopBar, codemirror family (3 chunks), ckeditor5 (3 chunks); @SCAN codemirror (23 chunks).
- [SRC] Annotations on content - @AM annotation-app, async-apps-annotation.
- [SRC] Approvals - see 3.7 (approval keys).
- [SRC] Media galleries and video playback - @NS gallery (24), lightbox (26), videoMessage (38); @K video_* (51 prefix); @AM ams-video-player, ams-video-player-engine, StreamEmbeddedRenderer, streaming-end-screen; VideoPlaybackStrings0 to 71 (locale bundles).
- [SRC] Teams gallery app - @AM teams-gallery-app, async-apps-teams-gallery.
- [SRC] Co-authoring infrastructure (Fluid) - @AM fluid-compose-container, fluid-message-container, fluid-paste-container, fluid-calendar-meeting-notes, fluid-thread-converged-notes-card, FluidDocumentServiceFactoryContainer; @NS ConnectionManager (18), OdspDriver (2), DeltaConnection (10), serializedStateManager (6); @SCAN yjs (61 chunks).
- [SRC] Sensitivity labels on files - @K sensitivity_update_file_label_not_found_description; @AM sensitivity-update, async-apps-sensitivity-update, apps-sensitivity-update.
- [SRC] Shared-with-team and shared-folder lists - see the P2P entries under Files above.

### 3.9 Realtime and sync architecture
- [SRC] Web real-time transport - @SCAN trouter (56 chunks), signalr (110), stomp (176), websocket (50), substrate (298); @AM SignalServiceForSubstrate; @INFRA ic3. (71 in the earlier pass).
- [SRC] Chat sync service - @K chatsync_app_service_disconnected (service name chatsync); @INFRA chatsvc (207 in the earlier pass).
- [SRC] Native calling transport (DTLS, SRTP, ICE, RTP) - ic3rustcore.dll (Rust DTLS and SRTP), skypert.dll (dtls_v2_openssl, dtls_key_cert_manager), RtmMediaManager.dll (mmiceserver, SDP parser), RTMPLTFM.dll (rtpsend, rtprecv, rtpred, rtpdtmf).
- [SRC] Local storage - sqlite3.dll; IndexedDB in web code (127 chunks, @SCAN); @NS UpdateLocalCacheSupportedPoliciesAndSettingsNamespaces; @MF publisher cache folder TeamsSharedConfig.
- [SRC] Offline and cache behavior - index.html runs a cache-chunks loader; the desktop build ships service-worker-noop.js.gz; @NS cw (5 chunks; service-worker install and integrity keys such as SW_INSTALL_DEFER_INTERVAL_KEY_NAME, SW_INTEGRITY_CHECK_KEY_NAME, SW_DYNAMIC_MANIFEST_FEATURE_KEY_NAME, SW_INSTALL_LOCALE_SETTING_KEY_NAME); @AM lazy-download-progress-svg, lazy-download-complete-svg, lazy-download-error-svg.
- [SRC] Connectivity and error states - @DJ connectivity_error_secondary_text; @DJ error_curtain_* (desktop); @NS errors (148; clear_cache_button_label, chatsync_app_service_disconnected).
- [SRC] Sync delta streams and optimistic chat state - @NS DeltaConnection (10), DeltaManager (0), serializedStateManager (6), ConnectionManager (18), ProtocolHandler (3), LocationRedirection (14); @AM fluid-* chunks.
- [SRC] Worker topology - @SRC @R/desktop-assets/workers (6 files: precompiled-web-worker x2, precompiled-webrtc-web-worker, precompiled-telemetry-web-worker, precompiled-editor-dedicated and shared index-readjuster workers for the editor index); serve-config worker entries add precompiled-shared-worker, precompiled-telemetry-shared-worker, precompiled-pluginless-encoded-streams and precompiled-wasmcv.
- [SRC] Encoded (insertable) media streams - @SRC pluginless-encoded-streams workers; @SCAN insertable (4 chunks). [INF] Likely used for end-to-end media encryption; not confirmed by key names (e2ee keys: 0 in @K).
- [SRC] Codecs and media pipeline - @SCAN opus (348), av1 (91), vp9 (40); @SRC RtmCodecs.dll (SLIQ rate control, NV12 shaders, capturer); SlimCoreWebview2.dll (DX11 BGRA and YUV shaders).
- [SRC] Push and background execution - @SRC PushNotifications.winmd, ApplicationModel.Background.winmd (plus UniversalBGTask), msteams_autostarter.exe; @SCAN serviceworker (34 chunks).
- [SRC] Multi-device identity - @SRC TeamsDeviceId and TeamsDeviceID strings in skypert.dll, RtmMediaManager.dll and SlimCoreWebview2.dll (binary tokens); @K device* (235, substring); @K devices_store_* (devices store banners).
- [SRC] Media auth tokens - @SRC SkypeToken, SkypeTokenProvider and SkypeTokenProviderV2 strings in native DLLs (binary tokens).
- [SRC] Telemetry and diagnostics transport - @SCAN sentry (236), applicationinsights (36), opentelemetry (8); @NT mstelemetry; @SRC precompiled-telemetry workers; @NS telemetry (15; diagnostic data viewer keys).
- [SRC] Cross-origin isolation and scale hints - @SRC serve-configs crossOriginIsolationVersion 4 (work and life); nine per-cloud endpoint sets (section 2.1).
- [INF] Call and media sharding by region - no direct key; per-cloud configs (@SRC) and the config chunk families (config-ag, config-bleu, config-delos, config-dod, config-gallatin, config-gcchigh, config-life, config-prod) are the nearest signals.

### 3.10 Governance, security and compliance
- [SRC] Sensitivity labels - @K sensitivity* (55 prefix), sensitivity_update_* (meetings and files); @AM InformationProtectionStrings0 to 85, sensitivity-update, async-apps-sensitivity-update.
- [SRC] DLP on links and content - @K dlp_blocked_link_receiver, dlp_blocked_link_sender, dlp_blocked_link_sender_learn_more, dlp_actions_aria_label (dlp* 44 prefix).
- [SRC] Retention and legal strings (features are server side) - @K retention* (12 prefix), legal* (5 prefix). [DOC-S] Retention and eDiscovery are Purview features.
- [SRC] Policies and admin settings - @K policy* (89, substring), admin* (68, substring); @NS UpdateLocalCacheSupportedPoliciesAndSettingsNamespaces.
- [SRC] Content moderation: report and appeal - @AM ReportConcernDialog, AppealConcernDialog; @K report_* (139, substring; communities_report_concern_menu_title, communities_join_flyout_report_concern_send, communities_report_concern_thank_you_dialog_auto_decline_text, app_details_report_review_alert).
- [SRC] Block contacts and privacy controls - @K privacy* (66 prefix; privacy_block_contacts, privacy_diagnostic_data_viewer, privacy_device_home_building_settings); @K contacts_action_block; @AM communities-block-members.
- [SRC] Diagnostic data viewer - @AM diagnostic-data-viewer (trusted UI); @NS telemetry (diagnostic_data_viewer_* keys).
- [SRC] Trusted UI and sign-in boundary - @AM trusted-ui index.html, auth-end-page-msa-tui-wrapper, authendpages, serviceBlocked, serviceBlockedDesktop, unsupportedPlatform; @SRC scwv2_bundle_trusted.js and bundletrusted.js (separate trusted bundles).
- [SRC] Sign-in confirmation and auth prompts - @K sign_in_confirmation_dialog_title, sign_in_confirmation_dialog_text, sign_in_confirmation_dialog_button; @DJ auth_prompt_title_sign_in, auth_prompt_title_acquire_token, basic_auth_dialog_caption, basic_auth_dialog_message.
- [SRC] Biometric data handling - @K recognition_export_biometrics_button_text, recognition_create_profile_button_text_v3, recognition_face_create_profile_button_text; @NS biometricEnrollment (67); @AM biometric-enrollment.
- [SRC] Captcha challenge flow - @AM captcha-challenge (async-apps-captcha-challenge); @NS PollOnDemandSupportedSettingNamespace (carries a subscribeToCaptchaChallenge string).
- [SRC] Sensitive-meeting protections - @K sensitivity_update_attendees_* (meeting sensitivity), watermark* (8, substring). [DOC-F] Premium protections include watermarking, sensitivity labels and recording restrictions; end-to-end encryption is listed in Premium material (DOC-S).
- [SRC] Tenant and region boundaries - @SRC serve-configs per cloud (ag08, ag09, bleu, delos, dod, gallatin, gcch, life, work); @AM config-ag, config-bleu, config-delos, config-dod, config-gallatin, config-gcchigh, config-life, config-prod; [DOC-F] Intelligent recap data stays in the tenant region and is multi-geo capable.
- [SRC] Error reporting for certificate, proxy and policy issues - @DJ error_curtain_body_cert_info, error_curtain_body_proxy_info, error_curtain_body_policy_info, error_curtain_body_corrupt_package, error_curtain_body_contact_it.
- [SRC] Test and mock content shipped - @AM tests-mock-auth, mocked-native-modules, platform-data.mock; @SRC configuration_gcc.json flag test/enableGCC (true).
- [SRC] Exploratory license blocking - @K exploratory_license_error_page_disabled_title, exploratory_license_error_page_expired_description_mf.
- [SRC] Window content protection (screen capture protection) - @SRC window_content_protection_manager_impl (ms-teams.exe source token).
- [SRC] Client firewall rules - @MF desktop2 FirewallRules for ms-teams.exe: inbound and outbound, TCP and UDP, all profiles.
- [SRC] Limited Access identity feature - @REA @PE named LIMITEDACCESSFEATURE IDENTITY resource in ms-teams.exe.
- [SRC] Trust indicators on shared and cross-posted content - see 3.1 and 3.2 (cross_post_trust_indicator_*, channel_card_trust_indicator).

### 3.11 Client shell (Windows desktop)
- [SRC] System tray menu - @DJ systray_menu_item_label_open, systray_menu_item_label_open_teams, systray_menu_item_label_quit, systray_menu_item_label_sign_out, systray_menu_item_label_settings, systray_menu_item_label_auto_start, systray_menu_item_label_do_not_auto_start, systray_menu_item_label_personal, systray_menu_item_label_close, systray_menu_item_label_log_support_collection, systray_menu_item_label_collect_dumps, systray_menu_item_label_open_dev_tools, systray_tooltip_activity.
- [SRC] Jump list (taskbar) - @DJ jumplist_menu_item_label_available, _busy, _do_not_disturb, _be_right_back, _away, _appear_offline, _reset_status, _unknown, _start_new_chat, _open_calendar, _schedule_meeting, _show_main_window, _not_teams_meeting, _all_day, _today_meetings; upcoming and no-meeting headers; @SRC jumplist icons (14 .ico).
- [SRC] Auto start at sign-in - @MF startup task TeamsTfwStartupTask (msteams_autostarter.exe); @DJ auto_start_enabled_toast_title, auto_start_enabled_toast_text, auto_start_toast_button_disable.
- [SRC] Auto-update and restart prompt - @DJ update_toast_title, update_toast_text, update_toast_button_intent_restartnow, update_toast_button_intent_remindmelater; @MF MSTeams.Update app (ms-teamsupdate.exe).
- [SRC] Error curtain and recovery - @DJ error_curtain_label_generic, error_curtain_body_generic, error_curtain_restart_button, error_curtain_repair_teams_button, error_curtain_open_teams_on_the_web, error_curtain_body_os_info, error_curtain_body_resource_info, error_curtain_label_runtime_version_not_supported, error_curtain_label_learn_about_edge_webview2.
- [SRC] WebView2 runtime checks - @DJ edge_not_installed_error, download_edge_button_label; @MF WebView2 min 128.0.2739.67.
- [SRC] Renderer crash recovery - @DJ unresponsive_renderer_toast_title, unresponsive_renderer_toast_text, unresponsive_renderer_button_restart, unresponsive_renderer_button_wait, unresponsive_renderer_button_trace, unresponsiver_renderer_button_reload.
- [SRC] Close prompt when leaving a call or chat - @DJ closeprompt_send_heading, closeprompt_send_text, closeprompt_send_leave_text, closeprompt_leave, closeprompt_stay, closeprompt_dont_show_again.
- [SRC] Deep links and protocol handlers - @MF protocols msteams, web+msteams, msteamscanary, callto, im, sip, sips, tel, action-sip, action-schedule; appUriHandler TeamsHosts with hosts teams.microsoft.com and teams.cloud.microsoft.
- [SRC] Share target - @MF "Send to Microsoft Teams" (any file type; storage items, text, bitmap).
- [SRC] COM servers - @MF toast activation callback 816EB0E0-F985-4919-BDA3-315878A4ED80 (ms-teams.exe); Office Integration 88435F68-FFC1-445F-8EDF-EF78B84BA1C7 (ms-teams.exe); ECS 'Ecs' class B8C1DCAE-5020-4F5D-BA2A-85292744E334 (ms-teamsupdate.exe).
- [SRC] Migration from classic Teams - @MF desktopAppMigration AumId MicrosoftTeams_8wekyb3d8bbwe!MicrosoftTeams.
- [SRC] Shared publisher config folder - @MF publisherCacheFolders TeamsSharedConfig.
- [SRC] Keyboard shortcuts and custom shortcut assignment - @K shortcut_* (283 prefix; shortcut_chat_compose_full, shortcut_files_new, shortcut_add_loop_paragraph, shortcut_accept_incoming_call_with_audio); @NS shortcuts (132; assign_custom_shortcut_input_label, assign_custom_shortcut_tooltip_win, cancel_shortcut_button); @K keyboard_layout_shortcuts_help_dialog_subtitle.
- [SRC] Themes and display - @K theme* (27, substring), dark* (27, substring); @SRC presence and jumplist icons with dark, light and high-contrast variants.
- [SRC] Accessibility - @K accessib* (28, substring), screen_reader* (8, substring), high_contrast (2); @NS teamsLabs (accessible message replies for screen readers).
- [SRC] Language and locale - @K language* (82, substring), locale* (5, substring); @SRC 63 locale folders and fallback-map.json.
- [SRC] Multi-window and pop-out - @K pop_out* (7; chatList_pop_out_new_chat_button_tooltip, embed_pop_out_chat, communities_pop_out_hover_text); @SRC window_placement_storage_* and window_cascade_impl (ms-teams.exe).
- [SRC] Window placement and sizing - @SRC window_placement_storage_multi, window_placement_storage_map, window_placement_storage_by_type, window_placement_storage_by_id, window_size_limiter_win.
- [SRC] Native notifications and Windows integration - @SRC WinMD list (section 1c).
- [SRC] Installer and setup UX - @SRC Msix.AppInstaller.Data/MsixAppInstallerData.xml (AppInstaller flyout); @SRC microsoft.system.package.metadata (empty folder).
- [SRC] Support tooling in the tray - @DJ systray_menu_item_label_log_support_collection, systray_menu_item_label_collect_dumps, systray_menu_item_label_open_dev_tools.
- [SRC] Desktop permission dialogs - @K app_device_permission_settings_dialog_browser_permissions_consistency_description; @AM screen-share-tab (trusted UI).
- [SRC] Hardware and device integration - @MF capabilities microphone, webcam, location, usb, humaninterfacedevice, bluetooth, radios, wiFiControl, voipCall, phoneCall; @SRC RtmPal.dll (HID, USB, Bluetooth, WASAPI, VoIP call coordinator).
- [SRC] Power management during calls - @SRC Microsoft.Windows.System.Power.winmd.
- [SRC] Windows AI actions registration - @MF windows.appExtension com.microsoft.windows.ai.actions MSTeamsActions (see 3.6).

### 3.12 UX patterns and layout
- [SRC] App bar and left rail - @AM appbar-control-flyoutbuttonbadge, appbar-control-flyoutsearchresults, appbar-control-lazydndcontext, appbar-control-lazyflyout, appbar-control-quickaccesstile, appbar-control-seealltile, appbar-control-suggestedapptile, appbar-control-tilecontextmenu; @K simple_collab_left_rail_* (left rail reordering); @K hamburger* (10, substring).
- [SRC] Chat list and compose pane - @K chatList_* (88), compose_* (294); @AM chat-list-apollo, compose-box-v2-renderer, compose-message.
- [SRC] Contextual panes - @K contextual_pane_header_actions_aria_label, contextual_pane_header_arrow_back_aria_label, contextual_pane_header_brand_aria_label, contextual_pane_header_dismiss_aria_label; @NS contextualHeaderPane (4), contextualPane (2).
- [SRC] Flyouts, dialogs and context menus - @K flyout_* (188 prefix), dialog* (27 prefix); @AM ContextMenu, CommandingSurfaceV9 (command surface component), DeleteConfirmationDialog, announcement-dialog-in-meeting.
- [SRC] Coachmarks and onboarding - @K coachmark keys (substring; ext_workflows_coachmark_*, collaborative_tabs_coachmark_*, agent_monitor_coachmark_*), onboarding* (12 prefix); @NS tflOnboarding (160), growth (162; calendar_empty_state_connect_google_bygc_button).
- [SRC] Empty states and activity-feed recommendation cards - @K empty* (73 prefix), activity_esp_* (chat, files, meeting, room insights, upgrade and contact-sync recommendation cards); @AM esp (async-apps-esp). The ESP acronym is not expanded.
- [SRC] Calendar app and calendar components - @AM calendar, async-apps-calendar, components-calendar-agenda-view-embed, components-calendar-app-host, components-calendar-main-slot-content, components-calendar-modal, components-calendar-multi-meeting-view-modal, bridge-migration-components-calendar; @SRC serve-configs endpoint google/oauthcallback (Google calendar sign-in).
- [SRC] Settings pages - @AM settings, async-apps-settings, accessibility-settings-panel, about-tab-renderer; @NS settings (472; analyze_content_experiences_*).
- [SRC] Help and what's new - @AM async-apps-help; @NS help (1), whatsNew (467).
- [SRC] Wellbeing features - @NS health (15; chair_exercises_heading, dismiss_button, explore_more_button_aria_label, disclaimer_Text).
- [SRC] Contact and profile cards - @NS livePersonaCard (51), contacts (167); @AM person, people-app-main-view.
- [SRC] Toolbars and controls in meetings - @AM meeting-controls-panel, presenter-toolbar-app, share-tray, video-effects-and-settings-panel.
- [SRC] Mobile-width message layout - @NS mobileAttach (124 keys); no mobile-layout chunk names found. [DOC-S] Mobile message layout changes in 2026 roundups.
- [SRC] Layout and view settings - @K view* (9 prefix); @AM layout (2 chunks), header (2 chunks).
- [SRC] Discover and slices - see 3.4 (discover-surface-*, headline and inline slices).
- [SRC] Sidebar sections and reordering - see 3.2 (simple_collab_*).
- [DOC-S] 2026 layout changes (monthly roundups): a Shared tab in Channels; a share panel with previews and an accidental-share safeguard; rearranged meeting controls; a setting that changes the Enter key on desktop and web; an Efficiency Mode for lower-spec devices.
- [INF] Unknown prefixes with UI meaning not confirmed: nearby (148 in @NS, mostly layout), svFlyout (180 in @NS), fai-EditorInput (158 in @NS, editor input component), podcast (483 in @NS, mostly code identifiers; no podcast keys in @K).

## 4. Extension points (plugin, skill, MCP, API, CLI, webhook surfaces)

Shell and OS surfaces:
- Deep links [SRC @MF]: msteams, web+msteams, msteamscanary (canary scheme), callto, im, sip, sips, tel, action-sip, action-schedule. App URI handler TeamsHosts covers teams.microsoft.com and teams.cloud.microsoft.
- Windows AI actions [SRC @AR, @MF]: extension com.microsoft.windows.ai.actions, id MSTeamsActions, registration action_manifest.json (version 2). Two actions: MSTeams.Action.Chat (action-sip URI built from a Contact input) and MSTeams.Action.Schedule (action-schedule URI). Both are shipped with isAvailable false and usesGenerativeAI false.
- Share target [SRC @MF]: "Send to Microsoft Teams" accepts any file type plus storage items, text and bitmaps.
- COM and type libraries [SRC @MF]: toast activator class (816EB0E0-F985-4919-BDA3-315878A4ED80), Office Integration class (88435F68-FFC1-445F-8EDF-EF78B84BA1C7), Ecs class (B8C1DCAE-5020-4F5D-BA2A-85292744E334, ms-teamsupdate.exe), and UC type libraries for x64, arm64 and x86 with conversation, contact, conference, participant, presence, sharing, device and meeting interfaces (identifiers only; methods not extracted).
- Launch surfaces [SRC @MF]: execution aliases ms-teams_ac.exe, ms-teamsupdate.exe, ms-teams_autostarter.exe; startup task TeamsTfwStartupTask.

Web app extension surfaces:
- Platform apps [SRC @AM, @K]: side panel, deep link into the side panel, app rail, subscriptions, consent containers (platform-app-* chunks; extensibility_* keys; lazy-extensible-app-with-consent-container).
- App picker and add-app dialog [SRC @K]: extensibility_app_picker_*, extensibility_addAppDialog*; scopes for personal apps, team tabs and chats (one-way bots, chat-with-subscription).
- Bots and message surfaces [SRC @K, @AM]: bot cards and blocked-reason strings, bot citations, message controls, Adaptive Cards (adaptivecard chunks), message extensions, slash commands, input extensions (app-based link previews).
- Agents [SRC @K, @NS, @AM]: project agents with knowledge resources (chat and meeting scope), channel agent, configuration agent (settings action types listed in section 3.7), agent details (permissions, stats), agent skills and preferences (nova namespace).
- Meeting and calling surfaces [SRC @AM]: meeting-apps-flyout-renderer, meeting-agent-settings-container, presenter and attendee apps, callable channel.
- Developer and configuration surfaces [SRC]: ecs_settings_editor.html (local ECS editor for rings: compare, apply, restart, load descriptions file), configuration_gcc.json (area/flag names), mock modules shipped in the web bundle (tests-mock-auth, mocked-native-modules, apps-platform-data.mock).
- Outlook [SRC @NT, @SRC]: MicrosoftTeamsMeetingAddinInstaller.msi and the outlooktma component; Office hand-off strings (officeaihandoffpluginstrings).
- Preview programs [SRC @NS]: Teams Labs (teamsLabs, 78 keys) and Teams Insider (teamsInsider, 94 keys) opt-in lists.
- Custom shortcuts [SRC @NS]: assign_custom_shortcut_* strings (user-defined keyboard shortcuts).

Developer platform (documentation):
- [DOC-F] Agents in Teams overview (Learn, updated July 2026): Teams SDK is the current SDK for agents in Teams; the Microsoft 365 Agents SDK extends those agents to other Microsoft 365 hubs such as Outlook; Microsoft 365 Agents Toolkit is the pro-code tool with Visual Studio Code and Visual Studio integration; Copilot Studio is the low-code route.
- [DOC-S] Teams SDK: JavaScript and C# generally available, Python in preview; adds Model Context Protocol and agent-to-agent support. TeamsFx is discouraged for new work. Teams Toolkit evolved into the Microsoft 365 Agents Toolkit (May 2025 developer blog).
- [DOC-S] App extension types: tabs, bots, message extensions (compose and search), meeting extensions and Adaptive Cards. Apps can surface in Outlook and Microsoft 365 Copilot.
- [DOC-S] App manifest schema: Learn cites version 1.17 as current; developer.microsoft.com also publishes schema 1.28. Check the live schema before choosing a version.
- [DOC-S] Graph API from Teams apps: sources are dated 2018 to 2021; treat as background only.
- [DOC-S] Workflows webhooks (Power Automate) replace the retired Office 365 Connectors; posting to private channels was added in 2026 (secondary source).
- [DOC-S] Copilot Studio can publish an agent to Teams and Microsoft 365 Copilot; Teams messages can be knowledge sources (2025 release plan).
- [DOC-S] Channel Agent: public preview, one per channel, created by default for new channels; the apps-permission setting controls who can create or delete it.
- [DOC-S] Microsoft 365 Agents SDK: one agent can serve several channels (Microsoft 365 Copilot, Teams, web).
- [DOC-S] Agent Builder in Microsoft 365 Copilot creates declarative agents; one Learn excerpt says those agents are not used in Teams Chat (conflict noted in section 8).

## 5. Data and protocol hints

- Local storage [SRC]: sqlite3.dll (native); IndexedDB (web, 127 chunks); policies and settings cache namespace (UpdateLocalCacheSupportedPoliciesAndSettingsNamespaces); publisher cache folder TeamsSharedConfig; trusted UI local-mru-files (recent files list); azure-storage-blobs.dll and azure-core.dll (Azure Blob SDK in native code).
- Wire formats [SCAN, SRC]: GraphQL with Apollo in the web client (graphql 242 chunks, apollo 155, resolver modules r_data-resolvers-*); JSON for manifests, configs, locale bundles and Adaptive Cards; SDP and ICE candidates for calls (RtmMediaManager: SDP session, media and table; mmiceserver); RTP, SRTP, RED and DTMF (RTMPLTFM); DTLS (ic3rustcore.dll; skypert.dll dtls_v2_openssl); codecs in use: Opus, AV1, VP9 (web scan) and SLIQ and NV12 pipelines (native).
- Real-time signalling [SCAN, AM]: SignalR, STOMP, WebSocket and Trouter in the web chunks; Substrate (service and chunk name SignalServiceForSubstrate); chat sync service name chatsync [K]; Yjs (61 chunks) for co-authoring.
- Auth [SCAN, NT, SRC, K]: MSAL in the web client (161 chunks); OneAuth in native code (NOTICE); Windows OAuth WinMD; SkypeToken and SkypeTokenProvider strings in native DLLs; tenant redemption and home-tenant strings in en-us keys; trusted UI auth end pages.
- Telemetry [SCAN, NT, SRC]: Sentry (236 chunks), Application Insights (36), OpenTelemetry (8) in web code; mstelemetry in native code (NOTICE); precompiled telemetry workers; diagnostic data viewer in the trusted UI.
- Configuration keys [SRC]: area/flag style (test/enableGCC, tma/meetingAddinEnableVirtualAppointment, app/piebatch, app/webrr). Serve-config endpoint keys per cloud: default, google/oauthcallback, live-stream-container.html, streaming-container.html, serviceworker.js, unsupported-browser and worker paths.
- ECS [SRC]: deployment rings and description files (ecs_settings_editor.html); the Ecs COM class is registered by ms-teamsupdate.exe.
- Host and service names [INFRA, weak; earlier pass, method not preserved]: skype (33,520 hits), onedrive (17,544), whiteboard (14,438), viva (13,693), substrate (8,713), trouter (6,222), planner (4,827), graphql (4,787), yammer (3,767), powerbi (3,470), dynamics (3,279), relay (2,289), signalr (1,853), websocket (1,595), msal (1,287), yjs (1,100), stomp (1,057), sentry (1,041), sydney (826; service codename, INF), chatsvc (207), ic3. (71), outlook.office (549), sharepoint.com (156), loop.office (116), forms.office (68), bing.com (143). These are name counts, not verified endpoints.
- Network posture [MF, SRC]: firewall rules for ms-teams.exe (TCP and UDP, inbound and outbound, all profiles); crossOriginIsolationVersion 4 in the work and life serve configs; WebView2 Evergreen runtime (minimum 128.0.2739.67).
- Device and OS integration [MF, SRC]: WASAPI audio, HID, USB, Bluetooth, Wi-Fi discovery (WinRT), LLDP, geolocation access (emergency calling), Windows Phone Link (phonelink_* keys).

## 6. Blockers and limits

1. Runtime is not observed. Launching or running the app is not allowed, so every claim is static (file, manifest, string, chunk or resource evidence).
2. REA JS analysis of client-bindings is partial: the analysis worker ran out of heap (2 GB cap) on 8 of 9 bundles during the semantic phase, so graph coverage is partial. trace_application_feature on the bridge factory name returned no match; REA states this is not evidence of absence.
3. REA binary sessions were avoided (shared active target). Native export tables and symbols were not extracted; native names come only from embedded source-path strings (grep -a).
4. The web client (about 206 MB of gzip chunks, minified) was covered by names, marker scans and key samples, not by reading chunk bodies.
5. en-us strings: the main locale chunk gives 16,366 keys. Per-feature locale chunks (audience-view-dialog, artifacts-platform, InformationProtectionStrings, VideoPlaybackStrings, dwsettingsdialogstrings, mcclocalizedstrings and others) are covered only by the noisy namespace scan.
6. Opaque names (sht, r_data, services, scaht, webrec, nova, esp, brb, tmp, sv flyout, fai) are not expanded; meanings are given only where marked INF.
7. Earlier-pass counts (@KD, @INFRA) have an unknown method; they are weak signals.
8. Server-side services (chat, calling back ends, Copilot, Substrate, compliance) are not visible locally. Features that exist only on servers are tagged DOC or INF.
9. Public documentation: three Learn pages were fetched; all other public claims come from search snippets or secondary sites (DOC-S), which disagree in places (section 8).
10. Native tools failed on scratch paths longer than 260 characters (python, node, jq). Work-around: Git Bash tools, stdin piping and the file writer for long text. No findings were lost.
11. Housekeeping: an empty file was created by mistake at C:/Users/ImaFernandesDaCosta/bin/.no-op (outside scratch) and was removed right away. No other writes outside scratch.
12. Type libraries (uc.tlb, uc-arm64.tlb, ucx86.tlb) were parsed for identifiers only.
13. MicrosoftTeamsMeetingAddinInstaller.msi was not opened (MSI internals not parsed).
14. Counts are chunk-file counts and key counts. They say nothing about user counts or usage.

## 7. REA usage log (what was actually run)

- inspect_pe_resources on ms-teams.exe: complete coverage, 431 resources, 867 entries examined (result cached as a tool-results file; queried with jq through stdin).
- inspect_pe_resources on ic3rustcore.dll: complete coverage, 1 resource (VERSION).
- analyze_javascript_application on client-bindings (directory, detail summary, record-and-continue, max heap 2048 MB, timeout 600000 ms): 9 files, partial coverage, 17 findings, heap exhaustion on 8 bundles.
- inspect_analysis_view on that evidence: summary (returned with the analysis) and the modules page (14 items: rt_js_bindings_bundle, calling_electron_bundle, scpie_vdi_bridge_bundle, scpie_vdi_webrr_bundle, scpie_vdi_sc_bundle, bundletrusted, bundle, scwv2_bundle_trusted, scwv2_bundle).
- trace_application_feature with seed pieSharedBridgeFactory (contains, case sensitive): no match; source graph partial.
- Not used: open_binary, binary_session, search_strings, list_strings, binary_overview, inspect_artifact, analyze_function (shared active target, or not needed for an inventory).

## 8. Conflicts, unverified items and confidence

- Classic Teams dates: search snippets said government clouds were carved out of the schedule. The fetched Learn page says end of availability has started for GCC, GCCH and DoD as well. The fetched primary page is used (DOC-F).
- Premium live translated captions: a secondary roundup lists them as Premium; the Premium admin page was not opened (DOC-S).
- Manifest version: Learn cites 1.17 as current; developer.microsoft.com publishes schema 1.28 (DOC-S).
- Shared channels and private-channel feature support differ between a university page and a vendor page (DOC-S).
- Agent Builder: one Learn excerpt says agents built with it cannot be used in Teams Chat, while the builder is reachable from Teams desktop and web (DOC-S).
- Office 365 Connectors retirement: secondary sources give a May 2026 rollout and note earlier deadlines that moved (DOC-S).
- Client architecture: "React UI on WebView2, moved from Electron" appears only in secondary sources (DOC-S). Local evidence agrees with WebView2 (minimum version, WebView2 loader, web bundles), but the React part is inferred from the chunk markers only (INF).
- Calling bundle name says electron (@skype/skype-calling-electron): a name, not a runtime fact (INF).
- Unexpanded prefixes: nova, esp, sht, brb, tmp, r_data, services, webrec, scaht, sv flyout, fai, jana (contacts, INF). Their meanings are unconfirmed.
- Copilot-named keys are numerous (424 substring hits). Not every Copilot string means a shipped feature; some are licence or campaign strings.
- The Teams SDK, Microsoft 365 Agents SDK and Channel Agent details are current as of the Learn pages fetched (2026-07). Newer behavior after that is not covered.

## 9. Sources

DOC-F (fetched and read in this run):
- learn.microsoft.com/en-us/microsoftteams/platform/agents-in-teams/overview (ms.date 2026-07-17).
- learn.microsoft.com/en-us/microsoftteams/teams-classic-client-end-of-availability (ms.date 2026-07-16; the older learn.microsoft.com/microsoftteams/new-teams-automatic-upgrade-announced URL redirects here).
- learn.microsoft.com/en-us/microsoftteams/privacy/intelligent-recap (updated 2026-08-05).

DOC-S (search snippets or secondary pages, not opened):
- Monthly Teams roundups for 2026: teams.handsontek.net (August 2026), windowsreport.com (June 2026), it.cornell.edu (July 2026), neowin.net (January to March 2026).
- Channel Agent: support.microsoft.com (how to create a channel agent for a Teams channel).
- Agent interactions (private and public): devblogs.microsoft.com/microsoft365dev (July 14, 2026).
- Copilot Studio Teams knowledge sources: learn.microsoft.com/en-us/power-platform/release-plan/2025wave1/microsoft-copilot-studio/use-teams-chats-as-knowledge-sources.
- Agent Builder: learn.microsoft.com/en-us/microsoft-365/copilot/extensibility/agent-builder.
- Microsoft 365 Agents SDK overview: learn.microsoft.com/en-us/microsoft-365/agents-sdk/agents-sdk-overview.
- Teams SDK: learn.microsoft.com/en-us/microsoftteams/platform/teams-sdk/.
- Microsoft 365 Agents Toolkit: learn.microsoft.com/microsoftteams/platform/toolkit/microsoft-365-agents-toolkit-cli; devblogs.microsoft.com/microsoft365dev (May 20, 2025 introduction).
- App manifest schema: learn.microsoft.com/en-CA/microsoftteams/platform/resources/schema/manifest-schema; developer.microsoft.com/json-schemas/teams/v1.28/MicrosoftTeams.schema.json.
- Shared channels: learn.microsoft.com/microsoftteams/shared-channels (page family) and the support.microsoft.com article on guests and shared channels.
- Premium overview: support.microsoft.com/office/overview-of-microsoft-teams-premium-4b34cd18-fa16-4797-bae0-b96b13a54f52; UC Today roundup of Premium features.
- Office 365 Connectors retirement: mc.merill.net/message/MC808160; itnext.io article on migrating from webhooks to Workflows.
- Classic Teams and new client background (secondary): petri.com, computerworld.com, m365admin.handsontek.net.
- DLP, retention and eDiscovery (secondary): practical365.com (configuring DLP in Teams); support.microsoft.com (information protection in Teams).

