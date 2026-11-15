# Codex desktop app: inventory (t3 / codexagent)

Audit: AUDIT 3, Model Plane and Verevon chat and agent pages. Source: Codex desktop app (OpenAI.Codex, MSIX). Role: inventory scout, breadth first, read only.
Scope: plugins and extensibility, agent runtime, model capability handling. The in-app browser and Chrome control belong to another audit and are only cross-referenced here.
Install root: C:/Program Files/WindowsApps/OpenAI.Codex_26.1007.2314.0_x64__2p2nqsd0c76g0/app/
Extracted tree (scratch, read only): scratchpad/extract/codex/

Provenance tags:
- REA = observed through REA analysis of a shipped artifact.
- SRC = read directly from files: manifests, SKILL.md, package.json, server.mjs, JS bundles, PE resources. [JS] = read-only grep or Node scan of the minified desktop bundles. [bin] = read-only grep -a string match on a native binary. Counts are occurrences, not proof of runtime behaviour.
- DOC = documentation (plugin docs/*.md, SKILL.md prose, plugin descriptions).
- INF = inference; the basis is stated.
Confidence: high, medium or low.

Rules followed: nothing was executed (no app, installer, dev server, test or build). No per-user data, credential, cookie, keyring or session store was opened. No secret values were read or recorded. Nothing outside the scratchpad was written.

Status (continuation pass): an earlier pass had written this file in full. This pass re-checked its counts, paths and wording against the installed and extracted files and made these corrections:

- Locale files: 64 native-menu locale files, not 60 (sections 2b and 3.12).
- IPC literals are in .vite/build bootstrap-487kJHw_.js, preload.js and browser-page-preload.js; get-browser-webmcp-policy also appears in main-BklS_2Y2.js (section 3.13).
- code-review has 28 pull_requests.* tools, not 24 (section 3.5).
- codex-app-tools approval prompts are set in its .mcp.json, not server.mjs (section 3.5).
- thread/rollback is in the JS bundles, not in codex.exe strings (section 3.1).
- tweak.md sits under skills/visualize/, and browser api.json sits under docs/ (section 2c).

Added section 3B (app-server method names from codex.exe) and new bullets in sections 3.1, 3.4, 3.5, 3.6, 3.8, 3.9, 3.10 and 3.11. Each added name matched a fixed-string search in codex.exe. Bullet count: 253 (section 3: 207; 3A: 34; 3B: 12 bullets, each grouping several names).

Re-checked and unchanged: non-vendored plugin file counts (52, 52, 9, 5, 6, 6, 19, 2, 17); install-root files and sizes; the Chromium 155.0.8059.40 manifest; owl-app.ini and owl-electron-app.json; 34 .vite/build files; 19,961 renderer assets (14,770 js, 2,362 svg, 33 wasm); 29 webview/apps icons; the push service-worker host allow-list; Google template ids (google-docs, google-slides, google-sheets); cua_node manifest and @oai/sky; node_repl turn_ended hooks on Interrupt, SubagentStop and Stop (browser, chrome, computer-use); setup-codex update_plan; hatch-pet 8x11 atlas; deep-research report-source.md and $documents; visualize CDN allowlist; REA summary statistics (section 7).

## 1. Identity

- Product: Codex (OpenAI) desktop app. Package OpenAI.Codex_26.1007.2314.0_x64__2p2nqsd0c76g0. App version 26.1007.21434, codexBuildFlavor prod, build number 13901. [SRC package.json]
- Shell: Electron. devDependency pin electron 42.3.0. Electron Forge 7.11.2 with msix, deb, rpm and zip makers, plus @electron/fuses. MSIX identity name OpenAI.Codex, publisher CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B. [SRC package.json]
- Chromium runtime: side-by-side manifest for Chromium 155.0.8059.40 in the install root. owl-app.ini sets UserDataDirectoryName=Codex. owl-electron-app.json sets runtimeName owl. [SRC] INF: a Chromium-based "owl" runtime hosts the Electron shell (medium).
- ChatGPT.exe (4.8 MB): carries INTEGRITY/ELECTRONASAR and LIMITEDACCESSFEATURE/IDENTITY resources and 6 icon groups. [REA inspect_pe_resources]
- Codex.exe (20 KB PE32+): one RT_MANIFEST resource and no icon groups. Treated as a launcher stub (INF, medium). [REA]
- Agent runtime: resources/codex.exe, 334 MB PE32+ x64, no PE resource table. [REA] It is a Rust workspace (INF from module paths, medium-high). Workspace crate roots seen in strings [bin]: codex_core, codex_app_server, codex_app_server_transport, codex_app_server_daemon, codex_tui, codex_rmcp_client, codex_exec_server, codex_api, codex_client, codex_http_client, codex_model_provider, codex_mcp, codex_core_plugins, codex_skills_extension, codex_memories_extension, codex_network_proxy, codex_otel, codex_utils_pty, codex_utils_absolute_path. Runtime version not determined (no VersionInfo strings).
- Linux build: resources/codex, 294 MB ELF64 (magic bytes checked). Not analysed. INF: the same runtime, shipped for Linux or remote hosts (low).
- Helper binaries in resources/: codex-code-mode-host.exe (75 MB, embeds V8: 1,043 "v8" string hits [bin]); codex-command-runner.exe (8 MB; JobObject, CreateRestrictedToken, ConPTY strings [bin]); codex-windows-sandbox-service.exe (56 MB) and codex-windows-sandbox-setup.exe (18 MB; CodexSandboxUsers, WFP, network_access strings [bin]); rg.exe (4.4 MB, ripgrep, not run); tectonic/tectonic.exe (LaTeX engine).
- Bundled Node runtime: resources/cua_node/ (node 24.21.0-cua.1, node_repl.exe, bin/node_modules including the @oai/sky computer-use package). [SRC cua_node/manifest.json]
- Desktop JS dependencies, grouped by capability [SRC package.json; runtime dependencies unless marked dev]:
  - Shell and packaging: electron 42.3.0 (pin); electron-context-menu 4.1; electron-windows-msix 2.0.4; @electron/fuses 1.8, @electron/notarize 3.1, @electron/windows-sign 1.2.2; Electron Forge 7.11.2 (cli, maker-msix, maker-deb, maker-rpm, maker-zip, maker-base, plugin-vite, plugin-auto-unpack-natives; dev); objc-js 1.5 (macOS native bridge); node-addon-api 8.9 and node-gyp 12.4 (native build; dev).
  - Agent and app-server bridge: workspace packages app-server-manager, app-server-types, protocol, shared-node, commands, external-agent-migration, git-worker, browser-common and codex-app-tools-plugin (not shipped as source); capnweb 0.11 (Cap'n Web RPC); vscode-jsonrpc 8.2 (JSON-RPC transport); vscode-languageserver-protocol 3.17.5 (LSP client); vscode-uri 3.1.0.
  - Browser-use libraries (other audit): browser-api, browser-backend-common and browser-client (linked); playwright 1.61 (dev; Electron agent CDP script).
  - Local state and data: better-sqlite3 12.11 (native); yjs 13.6.31 (CRDT); @tanstack/query-core 5.101 (client query cache); @oai/integrity-state; zod 4.4 (schemas); lodash 4.18; mime-types 2.1; tslib 2.8.
  - Processes, terminal and files: node-pty 1.1 (native PTY); @parcel/watcher 2.5.6 (file watching); extract-zip 2.0.1; tar 7.5.16; fs-extra 11.3.6 (dev); shlex 3.0 (shell quoting); bufferutil 4.0 and utf-8-validate 6.0 (WebSocket speed-ups).
  - Network and remote: ws 8.21 (WebSocket); ssh-config 5.2 (SSH host config); tldts 7.4.9 (domain parsing).
  - Config and formats: smol-toml 1.7 (config.toml); yaml 2.9; @napi-rs/canvas 0.1.89 (image canvas; dev).
  - Telemetry and crash reporting: OpenTelemetry api 1.9, core 2.2, sdk-trace-base 2.2, resources 2.2, otlp-transformer 0.208; Sentry core 10.29, node 10.29, electron 7.5; @sentry/cli 3.1 (dev).
  - Devices: @worklouder/device-kit-oai 0.2.1 and @worklouder/wl-device-kit 0.2.2 (file packages); native hid-topology-watcher.node.
  - Build and quality tooling (dev): vite 8.2.0; @rolldown/plugin-babel 0.2; typescript 7.0; vitest 4.1.10; cross-env 7.0; oxfmt and oxlint (scripts); @types/node 24.13, @types/ws, @types/lodash, @types/mime-types; fb-dotslash 0.5.
  - Native addons shipped unpacked [SRC app.asar.unpacked]: better-sqlite3, node-pty, @worklouder/device-kit-oai.
- Workspace packages referenced but not shipped as sources: app-server-manager, app-server-types, protocol, shared-node, commands, git-worker, external-agent-migration, browser-common, codex-app-tools-plugin. They are compiled into .vite/build. [SRC package.json] (high)
- Dev toolchain (not part of the shipped runtime): Vite 8.2.0 with rolldown, TypeScript 7.0, vitest 4.1.10, Playwright 1.61 (an Electron agent CDP REPL script), oxfmt and oxlint. [SRC package.json]
- Renderer library markers [JS, app-shared and app-initial bundles]: shiki 46, mermaid 42, three 38, remark 28, yjs 27, prism 12, katex 10, recharts 1, monaco 1, react-dom 2. INF: markdown with code highlighting, Mermaid and KaTeX diagrams and maths, and Three.js 3D views (medium; counts are noisy).

## 2. Directory and module map

2a. Install root [SRC listing]
- Codex.exe, ChatGPT.exe: launchers (see section 1).
- chrome.dll (329 MB), chrome_elf.dll, resources.pak, locales/, *.pak, icudtl.dat, v8_context_snapshot.bin, snapshot_blob.bin, libEGL.dll, libGLESv2.dll, vk_swiftshader.dll, vulkan-1.dll, d3dcompiler_47.dll, dxcompiler.dll: Chromium and Electron runtime.
- elevated_tracing_service.exe, elevation_service.exe, notification_helper.exe, chrome_proxy.exe, chrome_pwa_launcher.exe: Chromium-side helpers.
- resources/app.asar (567 MB): the Electron application (extracted to scratchpad/extract/codex/).
- resources/app.asar.unpacked/node_modules: @worklouder/device-kit-oai, better-sqlite3 build, node-pty build.
- resources/codex.exe (334 MB) and resources/codex (294 MB ELF): agent runtime.
- resources/codex-code-mode-host(.exe), codex-command-runner.exe, codex-windows-sandbox-service.exe, codex-windows-sandbox-setup.exe, rg(.exe): helpers.
- resources/native/*.node: computer-use-app-icons, hid-topology-watcher, remote-control-device-key, windows-account, windows-updater.
- resources/plugins/openai-bundled/: marketplace at .agents/plugins/marketplace.json, .bundle-id, and plugins/ (9 plugins).
- resources/plugin-signatures/openai-bundled/: per-plugin folders and receipt.json.
- resources/skills/skills/.curated/: hatch-pet and onboard-new-user (skill name setup-codex).
- resources/artifact-template-picker/: server.mjs and server.test.mjs.
- resources/cua_node/: Node runtime for Computer Use and node_repl.
- resources/tectonic/tectonic.exe (LaTeX); resources/default_app/ (stock Electron default app).
- resources/accessibility/: listed, not inspected.
- resources/codex-classic.wav, codex-notification.wav: notification sounds.
- resources/owl-app.ini, owl-electron-app.json, masque-proxy-origins.txt (lists https://codex-portals.api.openai.com), THIRD_PARTY_NOTICES.txt (3.3 MB; JavaScript notices only, no Rust crate names).

2b. Extracted tree: scratchpad/extract/codex/ (594 MB) [SRC]
- package.json: scripts and dependency manifest (section 1).
- .vite/build/: 34 JS files, 10.3 MB, all parsed by REA. main-BklS_2Y2.js (3.8 MB) is the main process. Others: early-bootstrap.js, preload.js, sandbox-preload.js, browser-page-preload.js, worker.js, service-DABseRGi.js, skills-8U-G2giZ.js, mcp-app-attachment-DibnvJgR.js, upload-BOGf8ZfH.js, windows-file-copy-BDdE9Xtu.js, desktop-open-path-queue-BbHRJtaX.js, record-aeon-assistant-message-delivery-B1sd4kI0.js, capture-Bk2D_sBl.js, character-preview-aWZT6FxB.js, startup-requirements-C6B1x9gL.js, build-flavor-CYXpoCFI.js, plus shared chunks (core-*, src-*, logger-*, dist-*, bootstrap-*, rolldown-runtime-*, zod-*).
- webview/ (561 MB per the earlier pass; not re-measured in the continuation pass): renderer. index.html (15 KB) and detached-window.html (3 KB). assets/ holds 19,961 files: 14,770 js, 2,362 svg, 937 webp, 870 json, 544 png, 317 css, 33 wasm, 67 font files, 19 wav, 6 ogg, 3 mp4, 3 each of xlsx, pptx and docx. Largest: app-initial (11.2 MB), app-shared (8.2 MB), two orbit-character WASM runtimes (6.7 MB and 6.6 MB), DocumentFormat.OpenXml WASM (4.4 MB), runtime.worker bundles (about 4.4 MB each), appshot-demo-*.mp4 (4.1 MB), and many per-locale bundles of 3 to 5 MB.
- webview/apps/: 29 icons for "open in" targets: Cursor, Windsurf, Zed, VS Code, VS Code Insiders, Antigravity, Devin, Android Studio, IntelliJ, PhpStorm, PyCharm, RustRover, GoLand, WebStorm, Rider, Xcode, Sublime Text, TextMate, BBEdit, Emacs, Finder, File Explorer, Terminal, Windows Terminal, Cmder, iTerm2, Warp, Ghostty, kitty.
- webview/service-worker/events.js: a ChatGPT-compatible push worker. Handles push types new-message and target_url. Allowed target hosts chatgpt.com and chatgpt-staging.com. [SRC]
- webview/licenses/ (emoji dataset licence), webview/images/ (ecosystem, visualize).
- node_modules/ (7 packages): @worklouder, better-sqlite3, bindings, file-uri-to-path, node-addon-api, node-pty, tslib.
- native-menu-locales/: 64 JSON menu locales, am through ur.

2c. Bundled plugins (resources/plugins/openai-bundled/plugins/; non-vendored file counts) [SRC]
- browser (52 files): skills/control-in-app-browser; scripts/browser-client.mjs, browser-service.mjs; docs/*.md and docs/api.json. In-app browser (other audit).
- chrome (52 files): skills/control-chrome; scripts including installManifest.mjs. Automation of the user's Chrome state.
- code-review (9 files): server.mjs (about 48k lines), launch script, .mcp.json (shipped with enabled false). GitHub and GitLab pull-request review as an MCP app.
- codex-app-tools (5 files): server.mjs (about 25k lines), launch scripts, .mcp.json (shipped with enabled false). Forwards desktop-app tools over a named pipe.
- computer-use (6 files): skills/computer-use; docs/guidance.md, api.md, confirmations.md. Windows computer use.
- deep-research (6 files): skills/deep-research with references/research-method.md, report-contract.md and agents/openai.yaml.
- latex (19 files): skills latex-compile, latex-doctor, texlive-runtime-installer; scripts (compile_latex.py, install_texlive.py, latex_doctor.py, detect_*.py); tests.
- unified-computer-use (2 files): .mcp.json for the cua_repl server. App-managed browser and computer runtime (shipped with enabled false).
- visualize (17 files): skills/visualize with scripts/render.py, tweak.md (under skills/visualize/), widgets/calendar.md, examples/calendar.html.

## 3. Feature candidates (grouped by taxonomy)

Each bullet gives the capability, its pointers, the provenance tag and confidence. Pointers are key names, tool names or file names as found.

### 3.1 Chat experience (taxonomy 1)
- Projects and projectless threads: renderer keys projectless (97 hits), project/create, project/read, thread/list. [JS, bin] SRC, high.
- Pinned threads and custom sidebar sections, including a cloud-tasks section, thread/section/move, sidebar-custom-sections-v3, app-server-custom-sections. [JS] SRC, high.
- Archive and unarchive threads: thread/archive, thread/unarchive, archived-threads, archived-chats. [JS] SRC, high.
- Fork a conversation: thread/fork, fork-conversation, /fork. [JS, bin] SRC, high.
- Rewind, revert and rollback with checkpoints: thread/revert, thread/rollback (rollback is in the JS bundles only: .vite/build/src-BJv5Mk7n.js and webview app-shared; not in codex.exe strings), checkpoint_seed_v1, checkpoint_delta, checkpoint-rejected, /rewind. [JS, bin] SRC, high.
- Resume a previous thread: thread/resume, /resume. [bin] SRC, high.
- Rename a thread: thread/name/set, thread/name/updated, /rename. [JS, bin] SRC, high.
- Search across threads and branches: thread/search, thread/searchOccurrences, searchChats, search-branches. [JS, bin] SRC, high.
- Steer a running turn and interrupt it: turn/steer, turn/interrupt, thread-follower-interrupt-turn. [JS, bin] SRC, high.
- Queued follow-up messages: thread/queue/add, thread/queue/start, thread/queue/changed, thread-follower-set-queued-follow-ups-state. [JS, bin, REA] SRC, high.
- Thread goals: notifications thread/goal/updated and thread/goal/cleared; model tool update_goal (10 hits). Semantics not read. [JS, bin] SRC, medium.
- Plan view and plan updates: update_plan tool, turn/plan/updated, /plan. [bin] SRC, high.
- Reasoning recap and cancelled-reasoning rows: reasoning_recap, reasoning_cancelled; config hide_agent_reasoning and show_raw_agent_reasoning. [JS, bin] SRC, high.
- Transcript item types and listing: agentMessage, mcpToolCall, imageView, task_list_item; thread/turns/list, thread/items/list, thread/timeline/list. [JS, bin] SRC, high.
- Thread attachments: thread/attachment/add, thread/attachment/list, thread/attachment/remove, thread/attachmentOwner/list. [JS, bin] SRC, high.
- Thread follower clients: other windows can submit command, file and permission approvals, user input and MCP elicitation responses (thread-follower-* and thread-stream-following-changed). [REA endpoint literals, JS] SRC, medium.
- Diff review with inline comments: turn/diff/updated, review-diff, diff_comments, review-patch, diffs-container. [JS, bin] SRC, high.
- Commit and branch UI: commit_scan, commit_group, committed, branch-exists. [JS] SRC, medium.
- Detached conversation window: webview/detached-window.html. [SRC] SRC, high.
- Hotkey window: hotkeyWindowHotkey and hotkey-window-home-placement. [REA] REA, medium.
- Native context menus and drag-out of files and links: IPC show-context-menu, start-file-drag, start-link-drag. [JS] SRC, high.
- Shared-object snapshot IPC: get-shared-object-snapshot. Semantics unknown; Yjs is a dependency. [JS] SRC, low.
- Conversational onboarding: electron-conversational-onboarding-completions-v1 and onboarding_conversation. [REA, JS] SRC, medium.
- Desktop notifications and push: notifications, push service worker with new-message and target_url types. [JS, SRC] SRC, medium.
- Notification sounds: codex-notification.wav and codex-classic.wav. [SRC] SRC, high.
- Link previews and page metadata fetches (og:title, og:image, twitter:image literals in endpoint nodes). [REA] REA, medium.
- Window state and tray: electron-main-window-bounds, tray icons chatgpt-tray-dark.ico and chatgpt-tray-light.ico. [REA, SRC] SRC, high.
- Thread metadata, settings and queue editing: thread/metadata/update, thread/settings/update, thread/queue/update and thread/queue/reorder (names only). [bin] SRC, high for names, low for behaviour.
- Thread prediction requests: thread/prediction/request (name only; purpose not read). [bin] SRC, low.

### 3.2 Multimodal and creation (taxonomy 2)
- Image generation: model strings gpt-image-2, gpt-image-1.5, gpt-image-1, gpt-image-1-mini; renderer keys image_gen, imageGeneration, image_generation_limit_reached; embedded imagegen system skill. [bin, JS] SRC, high for presence, medium for the tool contract.
- Image viewing tool view_image (9 hits); renderer keys imageView and image_card_v2. [bin, JS] SRC, high.
- Pets (hatch-pet skill): create Codex-compatible animated pets as an 8x11 sprite atlas with 9 standard animation rows and 16 look directions, packaged as spriteVersionNumber 2 with QA artifacts; visuals delegated to $imagegen. [SRC SKILL.md] DOC/SRC, high.
- Pet avatars in the renderer: avatar_type codex-pet with avatar_manifest {pet_id, snapshot}. [JS] SRC, medium-high.
- Avatar overlay and mini-orbit onboarding: electron-avatar-overlay-open, electron-mini-orbit-onboarding-account-ids; orbit-character WASM runtime (6.7 MB). Purpose INF (desktop companion overlay). [REA, SRC] SRC, medium.
- Realtime voice threads: thread source realtime_voice; thread/realtime/start, stop, appendAudio, appendSpeech, sdp, error, closed; thread/realtime/transcript/delta and done; thread/realtime/outputAudio/delta; realtime/listVoices; renderer keys voicePicker, voice.realtime, realtime_voice. [bin, JS] SRC, high.
- Realtime and transcription models: gpt-realtime-1.5, gpt-live-1-codex, gpt-4o-mini-transcribe. [bin] SRC, medium.
- Global dictation with hotkeys: globalDictationHotkey, globalDictationToggleHotkey; transcription-error UI. [REA, JS] REA, medium.
- Appshot: capture an application window and attach it to a thread through a hotkey (prepare_appshot_capture, load_appshot_capture_source, complete_appshot_capture, finish_appshot_capture, cancel_prepared_appshot_capture, set_appshot_hotkey, get_appshot_target, list_apps); webview/assets/appshot-demo-*.mp4. [REA, SRC] REA, high for existence, medium for behaviour.
- Artifacts: artifact-session, artifact-list, artifact_comment. [JS] SRC, high.
- Document, slide and sheet artifacts: the template picker offers docx, pptx and xlsx plus Google Docs, Slides and Sheets templates; work mode writes skills under CODEX_HOME/skills/remote-skills. [SRC server.mjs] SRC, high.
- Office file handling in the renderer: DocumentFormat.OpenXml WASM (4.4 MB). [SRC] SRC, high for presence, medium for use.
- Embedded system skills for office and PDF work: documents, presentations, spreadsheets, pdf. [bin] SRC, high for presence.
- Interactive visuals: charts, maps, diagrams, simulations, 3D models, data explorers and UI previews as HTML fragments; visualize content reference; CDN allowlist; render.py preview. [SRC SKILL.md, plugin.json] DOC/SRC, high.
- LaTeX: compile and doctor skills; bundled Tectonic first, TeX Live or MacTeX fallback, optional managed TeX Live installer. [SRC plugin.json, SKILL.md] DOC/SRC, high.
- Video: only a demo clip is bundled; no video generation observed. INF, medium.

### 3.3 Research and web (taxonomy 3)
- Deep research: multi-pass search, one dedicated research worker, conflict reconciliation, and a render-checked DOCX with footnotes; requires the $documents skill. [SRC SKILL.md, plugin.json] DOC/SRC, high.
- Web search tool with live and cached modes: web_search, web_search_mode, web_search_cached, web_search_request. [bin] SRC, high.
- Fuzzy file search for mentions: fuzzyFileSearch, fuzzy_file_search. [bin, JS] SRC, high.
- Connectors (named app integrations): connector_openai_orbit_user_channel, connector_openai_wallet, connector_openai_sites, connector_openai_automations, connector_openai_plugin_management; connector setup and auth elicitation; mcp-connect-sheet. [JS] SRC, medium (names only).
- Brand-icon map for integrations (for example SharePoint, PagerDuty, Datadog, SQLite, FastAPI). [JS] SRC, medium.

### 3.4 Agent runtime (taxonomy 4)
- Turn lifecycle: turn/start, turn/started, turn/completed, thread/started, thread/status/changed, thread/tokenUsage/updated. [bin, JS] SRC, high.
- Model-facing built-in tool names in the runtime strings (occurrence counts): unified_exec 65, apply_patch 61, request_user_input 55, code_mode 50, update_plan 30, spawn_agent 25, exec_command 21, tool_search 19, request_plugin_install 11, write_stdin 11, shell_command 11, list_mcp_resources 10, update_goal 10, view_image 9, list_mcp_resource_templates 8, web_search 7, wait_agent 6, send_input 4, read_mcp_resource 2, resume_agent 2, read_file 2, close_agent 1. [bin] SRC, high for names, medium for schemas (not read).
- Deferred tool loading and search: tool_search, omit_tools_from deferred (plugin .mcp.json), tools.fetch_uncached, tools.cache_write, tools.cache_publish, tool_suggest metadata. [bin, SRC] SRC, medium-high.
- Code mode: the model writes JavaScript that calls tools inside a V8 host (codex-code-mode-host.exe); strings code_mode [bin]; the browser skill text shows an "// @exec:" pragma with max_output_tokens for the outer exec call. [DOC] SRC/DOC, medium.
- Node REPL exposed as MCP tools: js, js_reset and turn_ended on servers node_repl (browser, chrome and computer-use plugins) and cua_repl (unified-computer-use). [SRC plugin.json, .mcp.json] SRC, high.
- Subagents: spawn, wait, send input, resume, close; roles and types (agent_role 26, agent_type 16); max_depth identifier; agent_jobs identifier; TUI agent manager (/agent, agents.*). Renderer keys subagentTitle and hasSubagentActivity; SubagentStop hook. [bin, JS, SRC] SRC, high for names, medium for behaviour.
- Batch agent jobs: only the agent_jobs identifier was found. CSV batch tool names were not found in strings. [bin] SRC, low.
- Deep research delegates the investigation to one research subagent (see 3.3). [DOC] DOC, high.
- Automations: local automation runner gated by Statsig config in_app_local_automation (accessPolicy local-automations); automation runs; inbox items; automation_update tool; pull-request-fix automation template. [JS, SRC] SRC, high for existence, medium for behaviour.
- Inbox items from model output: the automation prompt requires exactly one ::inbox-item remark directive per run; renderer handles inbox-items-create and inbox-items-changed. [JS] SRC, high.
- Cloud threads: thread source aeon with environment ids; thread/startAeon; cloud-aeon-sidebar-cache-v1; "codex cloud" command text. [JS, bin] SRC, medium (INF: cloud-hosted task threads).
- Context compaction: thread/compact/start, /compact, auto_compact_token_limit, auto_compact_fallback_prompt, auto_compact_fallback_buffer_tokens, context_window_reminder, context_window_guidance; PreCompact and PostCompact hooks. [bin] SRC, high.
- Hook events in the runtime strings: SessionStart, SessionEnd, UserPromptSubmit, PreToolUse, PostToolUse, PermissionRequest, PreCompact, PostCompact, SubagentStop, TurnStart, Notification, Interrupt. Plugin manifests also use Stop. [bin, SRC] SRC, medium-high.
- Hook handler types: mcp_tool (used by bundled plugins); command and agent_hook (strings). [SRC, bin] SRC, medium for command, low for agent_hook.
- Error and recovery signals: thread_not_found, permission_denied, checkpoint-rejected, rate-limit banners, auto-compact fallback. [JS, bin] SRC, low-medium (not traced).
- Plan-driven setup flow: setup-codex calls update_plan with four fixed steps before each interactive surface. [SRC] SRC, high.
- Evals and benchmarks: no eval harness in the shipped app; developer scripts only (memory:analyze-snapshot, e2e:computer-use-native-pipe, vitest). [SRC package.json] SRC, high.
- Background terminals per thread: thread/backgroundTerminals/list (name only). [bin] SRC, high for name, low for behaviour.
- Items injected into a running thread: thread/inject_items (name only). [bin] SRC, medium.
- Elicitation counters: thread/increment_elicitation and thread/decrement_elicitation (names only; likely pending user-input bookkeeping, INF). [bin] SRC, low.

### 3.5 Tools, plugins and extensibility (taxonomy 5)
- Plugin manifest .codex-plugin/plugin.json: name, version, description, author, keywords, skills directory, mcpServers file, inline hooks, and interface (displayName, shortDescription, longDescription, developerName, category, capabilities Interactive/Read/Write, defaultPrompt, composerIcon, logo, logoDark, brandColor, screenshots). [SRC] SRC, high.
- Marketplace file .agents/plugins/marketplace.json: entries with a local source path, policy installation AVAILABLE, policy authentication ON_INSTALL, and a category. [SRC] SRC, high.
- Plugin APIs: plugin/list, plugin/read, plugin/install, plugin/uninstall, plugin/connector; plugins/list, plugins/installed, plugins/cache, plugins/search, plugins/featured, plugins/suggested/codex, plugins/api_marketplace, plugins/marketplace; marketplace/add. [bin, JS] SRC, high.
- Plugin install requested by the model: request_plugin_install, plugin_install_suggestion, plugin_install_elicitation_sent; UI keys plugin-install, plugin_details, plugins-settings, plugin_suggestions. [bin, JS] SRC, high.
- Plugin signing: receipt.json lists archive size and SHA-256 per bundled plugin, algorithm RSA_PKCS1_SHA256, a production key id, purpose CODEX_DESKTOP_BUNDLED_PLUGIN_SIGNING, and a GitHub workflow reference. Runtime enforcement not read. [SRC] SRC, high for the artifact, low for enforcement.
- Nine bundled plugins (section 2c). [SRC] SRC, high.
- Skill format: SKILL.md with YAML front matter (name, description), body instructions, optional references/, scripts/, tests/, and agents/openai.yaml (interface: display_name, short_description, default_prompt). [SRC] SRC, high.
- Skill APIs and config: skills/list, skills/config/write, skills/changed; config skills.read, skills.list, skills.selected_skill_instructions, skills.shadow_selection, skills.truncated; $skill mentions (skill-mention, skillMention) and @plugin mentions (pluginMention, browser-skill). [bin, JS] SRC, high.
- Skill sources: embedded system skills written under CODEX_HOME/skills/.system (imagegen, openai-docs, skill-creator, skill-installer, pdf, documents, presentations, spreadsheets); remote skills under CODEX_HOME/skills/remote-skills; a GitHub skills-tree reference (skills/tree/main/skills/); skills/public. [bin, SRC] SRC, high.
- Skills that call other skills by name: hatch-pet uses $imagegen; deep-research uses $documents. [DOC] DOC, high.
- MCP client: rmcp-based, with stdio and HTTP transports (codex_rmcp_client, http_client_adapter, stdio_server_launcher, event-notification translator). [bin] SRC, high.
- MCP server lifecycle: mcpServerStatus/list, mcpServer/startupStatus/updated, mcpServer/oauthLogin/completed, mcpServer/resource/read, mcpServer/elicitation/request. [JS, bin] SRC, high.
- MCP per-server config fields: command, args, cwd, enabled, env_vars allowlist, startup_timeout_sec, tool_timeout_sec, enabled_tools, omit_tools_from, default_tools_approval_mode, tools.<name>.approval_mode, tools.<name>.output_token_limit; mcp_servers in config.toml. [SRC plugin .mcp.json, bin] SRC, high.
- MCP Apps: tool results can carry UI resources rendered in a sandboxed host and guest pair (IPC mcp-app-sandbox-host-message and mcp-app-sandbox-guest-message; mcp_app_sandbox.init_rejected; mcp-app-attachment chunk). code-review sets _meta.ui.resourceUri on its tools. [JS, bin, SRC] SRC, high.
- Bundled MCP server code-review: 28 pull_requests.* tools (account, search, authoredUpdates, mergeability, diff, stack, reviewSnapshot, revisionDiff, revisionFile, userSearch, media, avatar, update, merge, submitReview, comment, commentUpdate, reviewThreadUpdate, inboxDetails, localReview, startReview, chatReview, cancelReview, setReviewFindingResolved, checks, filter, open, settings), with GitHub and GitLab providers. Shipped with enabled false. [SRC] SRC, high.
- Bundled MCP server codex-app-tools: proxies desktop-app tools through the pipe named in CODEX_APP_TOOLS_PIPE_PATH (tools/list and tools/call). Approval prompts for create_thread, send_message_to_thread, fork_thread, handoff_thread, move_thread_to_project and automation_update, set in .mcp.json (default_tools_approval_mode approve; per-tool approval_mode prompt). Shipped with enabled false. [SRC] SRC, high.
- Bundled MCP server unified-computer-use (cua_repl): tools js, js_reset and turn_ended; output_token_limit 25000 for js. Shipped with enabled false. [SRC] SRC, high.
- Plugin hooks call MCP tools on Interrupt, SubagentStop and Stop events (node_repl.turn_ended, cua_repl.turn_ended). [SRC] SRC, high.
- Connectors: connector ids, connector setup and auth elicitation. [JS] SRC, medium.
- TUI slash commands in the runtime strings: /plugins, /skills, /mcp, /apps, /hooks, /memories, /permissions, /plan, /compact, /review, /fork, /resume, /new, /clear, /diff, /status, /model, /fast, /ps, /stop, /agent, /experimental, /feedback, /logout, /init, /rename, /statusline, /rewind, /side. [bin] SRC, high.
- Custom instruction files: AGENTS.md (73 hits), AGENTS.override.md, project_doc_fallback_filenames, project_doc_max_bytes. [bin] SRC, high.
- Config files: config.toml (user level, and project level .codex/config.toml), profiles, features and experimental_features, model_providers, tui.keymap, notify, otel, shell_environment_policy. [bin] SRC, high.
- App-server JSON-RPC surface (v2-style method names): thread/*, turn/*, review/start, command/exec, fs/* (readFile, writeFile, createDirectory, copy, remove, getMetadata), config/* (read, write, batchWrite, bundle), account/* (login/start, logout, read, rateLimits/read, rateLimitResetCredit/consume, workspaceMessages/read), model/list, feedback, realtime/*, hooks/list, project/*, skills/*, plugin/*, marketplace/add. [bin] SRC, high.
- Schema export for SDK clients: generate-ts, generate-json-schema, generate-internal-json-schema, app-server-test-client. [bin] SRC, high.
- App-server transports: stdio://, unix://, ws://, plus --listen and --remote flags. [bin] SRC, high.
- Code review as a built-in command: /review, review/start, review_model setting. [bin, JS] SRC, high.
- WebMCP policy for site-exposed tools: IPC get-browser-webmcp-policy; webmcp.md in the browser plugin docs. Other audit. [JS, DOC] SRC/DOC, high.
- Plugin sharing and reconciliation: plugin/share/updateTargets, plugin/share/checkout, plugin/skill/read, plugin/reconcile and plugins/workspace (names only). [bin] SRC, high for names.
- Marketplace upgrade: marketplace/upgrade, next to marketplace/add. [bin] SRC, high for name.
- Experimental flags and permission profiles listed to clients: experimentalFeature/list and permissionProfile/list. [bin] SRC, high for names.
- MCP server events and control: mcpServer/event/stream, mcpServer/startupStatus/updated, mcpServer/reload, mcpServer/tool/call and mcpServer/resource/read. [bin] SRC, high for names.
- Skill roots: skills/list accepts extraRoots; skills/config/write. [bin] SRC, high for names.

### 3.6 Computer and environment control (taxonomy 6)
- Computer Use on Windows: SendInput, UI Automation and Windows.Graphics.Capture screenshots; per-app allow-list; confirmation rules; target-window guidance. [DOC] DOC, high.
- Node-based @oai/sky computer-use API inside cua_node; computer-use-app-icons.node native module. [SRC] SRC, high.
- In-app browser and Chrome control (other audit): browser and chrome plugins; IPC browser-page-event and browser-sidebar-runtime-message; browser annotation policy IPC; CDP calls Debugger.enable, Debugger.setPauseOnExceptions, Runtime.getProperties; getUserTabs and focusTab endpoints. [REA, JS] REA, high.
- Shell and PTY: unified_exec, exec_command, write_stdin, shell_command; PTY via ConPTY strings in the command runner and node-pty in the desktop app. [bin, JS] SRC, high.
- Command runner: job objects for process trees, restricted token creation, timeouts. [bin] SRC, high.
- Sandbox policy: read-only, workspace-write (network_access toggle), danger-full-access. [bin] SRC, high.
- Windows sandbox service: CodexSandboxUsers, CodexSandboxOnline and CodexSandboxOffline user strings; WFP firewall rules; read ACL; sandbox_users config; setup executable. [bin] SRC, high.
- Exec policy rules: default.rules, prefix_rule, network_rules, justification, exec_policy. [bin] SRC, high.
- Client filesystem API: fs/readFile, fs/writeFile, fs/createDirectory, fs/copy, fs/remove, fs/getMetadata. [bin] SRC, high.
- Remote SSH hosts: remote_ssh_connections; ssh-config dependency. [JS, REA, SRC] SRC, high.
- Remote WSL hosts: remote_wsl_connections. [JS, REA] SRC, high.
- Remote control of this desktop from another client: remote_control_connections, enrollments (electron-remote-control-client-enrollments), local_remote_control_environment_id and installation id; remote-control-device-key.node. [REA, SRC] REA, medium-high.
- Remote-hosted picture-in-picture video frame channel: remote-hosted-pip-video-frame. [JS] SRC, medium.
- Remote exec server and environments: codex_exec_server crate; exec-server strings (476 hits); environment_id (28 hits). [bin] SRC, medium.
- Worktrees: worktree_pending_create, worktree-ready, worktree-owner, worktree_archive_cleanup, archived_worktree, remote-worktree, electron-managed-worktree-archives. [JS, REA] SRC, high.
- Open-in targets for IDEs, terminals and file managers (section 2b, webview/apps). [SRC] SRC, high.
- LSP client dependency: vscode-languageserver-protocol 3.17.5. Usage not traced. [SRC package.json] SRC, medium.
- Hardware device kit: @worklouder device-kit packages and hid-topology-watcher.node. Purpose INF. [SRC] SRC, medium.
- Network proxy: codex_network_proxy crate, network_proxy strings; masque-proxy-origins.txt. [bin, SRC] SRC, medium.
- Linux desktop-entry code paths present (Exec, TryExec, Icon, OnlyShowIn, DBusActivatable strings). [REA] REA, medium.
- Auto update: IPC check-for-updates; windows-updater.node. [JS, SRC] SRC, high.
- Environments: environment/add and environment/info (environment ids for local, remote or cloud runs; see environment_id above). [bin] SRC, high for names.
- Remote control pairing and devices: remoteControl/pairing/start, remoteControl/pairing/status, remoteControl/enable, remoteControl/disable and remoteControl/client/list. [bin] SRC, high for names.
- File watching on the client API: fs/watch (name only). [bin] SRC, high for name.

### 3.7 Coding capability (taxonomy 7)
- Patch application: apply_patch tool; turn/diff/updated. [bin, JS] SRC, high.
- Code review: /review, review/start, review_model; GitHub and GitLab review providers in code-review (localReview, startReview, chatReview, cancelReview, setReviewFindingResolved; private review without a checkout). [bin, SRC] SRC, high.
- Pull-request automation and PR chips: pull_request_fix_automation, sidebar_task_pr_chip, pull_request_link, task_pr. [JS] SRC, medium.
- Diff review with inline and patch comments; file approval decisions (thread-follower-file-approval-decision). [JS, REA] SRC, high.
- Run and test loops: achieved through the exec tools; no dedicated test-runner tool observed. INF, medium.
- Codebase indexing or embeddings: not observed. INF, low.
- Tab completion: not observed. INF, low.
- IDE integration: open-in launchers only; LSP client dependency; no IDE extension in the package. [SRC] SRC, medium.

### 3.8 Memory, learning and personalisation (taxonomy 8)
- Memory subsystem: memory_mode (35), memory_summary (50), memory_consolidation, memory_consolidate_global, memory_versions, memory_root, memory_import, memory_status, memory_readiness, memory_citation, memory_extensions_root, memory_extensions_primary_inputs, memory_extensions_folder_structure, memory_operators; config memories (164 hits); codex_memories_extension crate. [bin] SRC, high for names, medium for behaviour.
- Memory import from another agent: the strings CLAUDE.md, "claude code", claude-code, claude_code and claudecode appear near the memory import path. [bin] SRC, medium.
- External agent config import: SQLite table external_agent_config_imports; TUI module external_agent_config_migration; external-agent-migration workspace package. [bin, SRC] SRC, high for presence, medium for scope.
- Personality setting: personality (21 hits; value friendly seen). [bin] SRC, medium.
- Skill authoring: embedded skill-creator and skill-installer system skills. [bin] SRC, high for presence.
- Onboarding personalisation: setup-codex asks for role or roles, then a first task, then creates a first artifact. [SRC] SRC, high.
- Thread read-state tracking: electron-thread-read-state-v1 and thread-read-state-changed. [REA, JS] SRC, medium.
- Knowledge bases and retrieval over user documents: not observed. INF, low.
- Memory RPC: memory/status and memory/reset (names). [bin] SRC, high for names.
- External agent config import RPC: externalAgentConfig/detect, externalAgentConfig/import and externalAgentConfig/import/recordHistory, alongside the external_agent_config_imports table. [bin] SRC, high for names.

### 3.9 Models and routing (taxonomy 9)
- Model slugs present in the runtime strings: gpt-5.6-terra, gpt-5.6-luna, gpt-5.6-sol, gpt-5.5, gpt-5.4, gpt-5.3-codex, gpt-5.2-codex, gpt-5.2, gpt-5.1-codex-max (migration prompt), gpt-4.1, gpt-6-astra, gpt-6-sol, gpt-6-luna, gpt-6-pro, gpt-6.1-sol, codex-auto-review, gpt-oss. [bin] SRC, medium (catalog not confirmed; some names may be stale or placeholders).
- Model metadata and picker: model/list, selectedModel, model-migration prompt strings. [JS, bin] SRC, high.
- Reasoning effort: reasoning_effort (35), minimal (37), xhigh (13). [bin] SRC, high for the control, medium for the levels.
- Reasoning summary and verbosity: model_reasoning_summary, model_verbosity, hide_agent_reasoning. [bin] SRC, high.
- Service tiers and fast mode: service_tier with flex and priority strings (noisy counts); /fast; fast_mode; IPC get-fast-mode-rollout-metrics. [bin, JS] SRC, high.
- Providers and routing: model_provider and model_providers tables; wire_api (14 hits; values not confirmed); chatgpt_base_url; azure strings (7). [bin] SRC, high for strings, medium for behaviour.
- Local and OSS models: oss_provider with ollama (31 hits) and LM Studio (lmstudio, 16 hits). [bin] SRC, high for strings.
- Responses API details: encrypted_content, previous_response_id, a zdr marker. [bin] SRC, medium.
- Web search modes: live and cached. [bin] SRC, high.
- Structured output: JSON schema export of the protocol for SDK clients. [bin] SRC, high.
- Usage and rate limits: account/rateLimits/read, account/rateLimitResetCredit/consume, rate-limit-status UI, credits_warning, usage_limit, usage-settings, sidebar_usage_alert. [JS, bin] SRC, high.
- Token accounting: thread/tokenUsage/updated; post_sampling_token_estimate. [bin] SRC, high.
- Prompt-cache controls: prompt_cache_key not found in strings. INF, low (not observed).
- Model-specific prompt file: gpt-6-astra.md. [bin] SRC, low.
- Model slug presence (fixed-string matches in codex.exe): gpt-5.6-terra, gpt-6-astra, gpt-6.1-sol, codex-auto-review. Presence only, not a catalogue. [bin] SRC, medium.

### 3.10 Transparency, observability and control (taxonomy 10)
- Token and usage meters in the UI: tokenUsage updates, context-window reminders, usage alerts. [JS, bin] SRC, high.
- Tool-call transcript with MCP and executed-tool-call records (executed tool call truncation origin; mcp_tool_call UI). [bin, JS] SRC, high.
- OpenTelemetry: @opentelemetry packages; codex_otel crate; otel config (91 hits); trace headers traceparent and tracestate; x-request-id and x-oai-request-id. [SRC, REA, bin] SRC, high.
- Crash reporting: Sentry (electron, node and core 10.29); sentry.sample_rate and sentry.dsc; IPC get-sentry-init-options and trigger-sentry-test. [SRC, REA] SRC, high.
- Feature gates: Statsig evaluations and defaults (statsig_evaluations, statsig_default_enable_features, configFeatures entries such as in_app_local_automation). [REA, JS] REA, high.
- Feedback submission: /feedback and the feedback request processor. [bin] SRC, high.
- Status and process views: /status, /ps, /stop. [bin] SRC, high.
- Status line customisation: /statusline and tui.* settings. [bin] SRC, high.
- Notify hook: config notify (an external program). [bin] SRC, medium.
- Debug tooling: CDP Debugger and Runtime domains used from the main process. [REA] REA, high.
- Audit trail: no dedicated audit log observed. INF, low.
- Feedback upload RPC: feedback/upload (behind the /feedback command). [bin] SRC, high for name.

### 3.11 Safety, permissions and governance (taxonomy 11)
- Approval policies: untrusted, on-request, unless-trusted, never. [bin] SRC, high.
- Approvals reviewer: approvalsReviewer setting (value user seen); guardian identifiers in the binary (function not read). [JS, bin] SRC, low-medium.
- Permission requests: PermissionRequest hook; permissionRequest UI; permissionProfile/list; permissions-request-approval-response; permission_denied. [JS, bin] SRC, high.
- Sandbox and network policy: see 3.6 (sandbox modes, WFP rules, network_access). [bin] SRC, high.
- Exec policy rules (.rules files): see 3.6. [bin] SRC, high.
- Trusted-contact identifier present; semantics unknown. [JS] SRC, low.
- Plugin authentication policy ON_INSTALL on every bundled plugin. [SRC] SRC, high.
- Browser and computer safety docs: browser-safety.md and confirmations.md. [DOC] DOC, high.
- Credential storage: OS keyring (42 hits). Secrets not read. [bin] SRC, high for presence.
- Login paths: ChatGPT OAuth with PKCE, device-code auth, refresh and id tokens, API-key login (ApiKey). [bin] SRC, high for strings.
- Managed and enterprise settings: plugins.org config, workspace-team and workspace settings, managed plugin ids synced to the Chrome extension. [JS, REA, bin] SRC, medium.
- Zero-data-retention marker present (zdr). [bin] SRC, low.
- Prompt-injection stance: deep-research treats instructions in retrieved pages as untrusted source text. [DOC] DOC, high.
- Data-use notice in the browser and chrome plugin manifests: browser data may be used for training, subject to account controls. [SRC] SRC, high.
- Guardian-denied actions: thread/approveGuardianDeniedAction (the name suggests a user override of a guardian denial; behaviour not read); thread/shellCommand is nearby. [bin] SRC, medium.
- Permission profiles listed to clients: permissionProfile/list (see 3.5). [bin] SRC, high for name.

### 3.12 UI/UX and product design (taxonomy 12)
- Layout: sidebar (projects, pinned, threads, custom sections, archive); transcript; composer with mentions (plugins, skills, browser tabs, connectors, slash commands); side panel; browser sidebar; diff panel; artifact view. [JS] SRC, high.
- Global shortcuts: hotkey window, Appshot and dictation hotkeys. [REA] REA, high.
- Avatar overlay and pets. [SRC, REA] SRC, medium.
- Theme: system theme variant IPC (get-system-theme-variant, system-theme-variant-updated). [JS] SRC, medium.
- Keyboard: TUI keymap (tui.keymap, 16 hits). [bin] SRC, high.
- Accessibility: browser accessibility docs and a WASM accessibility module; resources/accessibility not inspected. [DOC, SRC] SRC, medium.
- Localisation: 64 native menu locale files; renderer locale bundles for a similar set (for example my-MM, ta-IN, ka-GE, bn-BD). [SRC] SRC, high.
- Onboarding: setup-codex four-step flow (role, first task, first artifact); conversational onboarding flag. [SRC, REA] SRC, high.
- Responsive or mobile layouts: no responsive CSS or layout code was checked; a /codex-mobile route string exists (see 3A). INF, low.
- Command palette: confirmed by codex.command.* locale keys (see 3A). Virtualisation and performance budgets: not confirmed. INF, low.
- Tray and window state: tray icons and persisted window bounds. [SRC, REA] SRC, high.

### 3.13 Platform architecture (taxonomy 13)
- Process model: Electron main (.vite/build/main-*.js), renderer (webview), preload scripts, workers, and child processes (codex.exe app-server, command runner, code-mode host, sandbox service, cua_node Node REPL). [SRC, REA] SRC/REA, high for the split, medium for spawn arguments (not read).
- IPC channel literals found in the .vite/build bundles (bootstrap-487kJHw_.js, preload.js and browser-page-preload.js; get-browser-webmcp-policy also in main-BklS_2Y2.js): codex_desktop:message-for-view, message-from-view, chunked-message-ack, worker:*, connect-app-host, get-initial-sidebar-bootstrap, get-build-flavor, get-system-theme-variant, system-theme-variant-updated, get-shared-object-snapshot, get-sentry-init-options, get-fast-mode-rollout-metrics, get-browser-webmcp-policy, get-browser-annotation-api-policy, browser-page-event, browser-sidebar-runtime-message, mcp-app-sandbox-host-message, mcp-app-sandbox-guest-message, remote-hosted-pip-video-frame, show-context-menu, start-file-drag, start-link-drag, check-for-updates, trigger-sentry-test. [JS] SRC, high.
- IPC statistics (REA): 38 operations; 36 use dynamic channel names that REA could not resolve; 2 literal channels; 31 renderer transmissions; 7 renderer listeners; 15 sender-validation sites; 3 context-bridge APIs; 0 utility processes; 0 native addon bindings resolved. REA, high.
- Message bridge with chunking and acknowledgements: chunked-message-ack, message-for-view and message-from-view. [SRC, REA] SRC, high.
- Protocols: app-server JSON-RPC over stdio, unix socket or websocket; MCP over stdio or HTTP; named pipe for codex-app-tools; Cap'n Web RPC; vscode-jsonrpc. [bin, SRC] SRC, high.
- Storage: better-sqlite3 in the desktop app; SQLite tables in codex.exe (external_agent_config_imports); config.toml; CODEX_HOME folders (skills, plugins/cache, memories); Electron persisted atom state. Contents not read, by rule. [SRC, bin] SRC, high for presence.
- Distribution and updates: MSIX via electron-forge maker-msix and electron-windows-msix; windows-updater.node; check-for-updates. [SRC] SRC, high.
- Plugin signature chain: RSA_PKCS1_SHA256 receipts per bundled plugin. [SRC] SRC, high.
- Rust-portable design notes: JSON-RPC method taxonomy per domain; a tool registry with deferred search; code mode as a sandboxed JavaScript orchestrator of tools; a three-level sandbox plus rule-based exec policy; plugin bundles carrying skills, MCP servers and hooks; progressive-disclosure skills (SKILL.md plus references and scripts); a memory consolidation pipeline; a thread model that followers can attach to. INF, medium.

### 3A. Second pass: route and locale scan (renderer bundles and native-menu locale keys)

Source: renderer path literals in app-shared and app-initial, and the English key names in native-menu-locales (keys only, no translations read). Routes show features that exist in the UI or its API; they do not prove behaviour.

Chat experience (taxonomy 1):
- Conversation sharing: share routes /share/create, /share/v2/create, /share/post and /share/e/:id. [JS] SRC, high (routes), medium (behaviour).
- Conversation organisation and search routes: /conversations/search, /conversations/batch (batch actions), /archive, /projects, /conversation/new_branch, /conversation/resume, /conversation/init. [JS] SRC, high.
- Temporary chat: command key codex.command.temporaryChat. [JS locale keys] SRC, high.
- Space deep links: /space/ and /codex/app/space/ URLs are parsed into an id and a blockId. Meaning INF (block-addressable space content); relevant to the Verevon Space question in AUDIT 1. [JS] SRC, medium.

Multimodal and creation (taxonomy 2):
- Pets UI routes: /pets and /pets/create. [JS] SRC, high.
- Image page route: /images. [JS] SRC, high (route), medium (features).
- Voice pages and settings: /voice, /settings/voices, dictation stream connect info (/codex/dictation-stream-connect-info). [JS] SRC, high.

Research and web (taxonomy 3):
- Promotions and checkout: /codex/p/:promoCode, /checkout/web and a credits nudge email route. [JS] SRC, high.

Agent runtime (taxonomy 4):
- Automation editor and API routes: /automations, /automations/save, /automations/set_status, /automations/remove; inbox page /inbox. [JS] SRC, high.
- Cloud environments and cloud agents: /cloud-environments/new, /cloud-aeons/primary, conversation routes /ca/:conversationId and /c/:conversationId, and /onboarding/aeon-initialization. [JS] SRC, medium.
- Cloud agent avatars: a PNG avatar is posted as multipart form data to /cloud-aeons/avatar-snapshots and a snapshot is returned. The pet avatar_manifest uses the same snapshot idea. Relationship INF (medium). [JS] SRC, medium.

Tools, plugins and extensibility (taxonomy 5):
- Plugin and skill UI routes: /plugins, /plugins/:pluginId, /plugins/:pluginId/app/:toolName, /plugins-redirect, /skills, /skills/manage, /skills/plugins, /settings/plugins, /settings/plugins-settings, /settings/mcp-settings, /mcp-app. [JS] SRC, high.
- Per-host plugin settings: /settings/plugins-settings/hosts/:hostId/plugins/:pluginId (plugins configured per remote host). [JS] SRC, high.
- Plugin and runtime update channels: /codex/app/updates/plugins/ and /codex/app/updates/runtimes/ (runtimes updated separately from the app shell). [JS] SRC, medium.
- Connector library route: /connector_openai_library/. [JS] SRC, high (route).

Computer and environment control (taxonomy 6):
- Connection settings: /settings/connections, /settings/connections/ssh, /settings/connections/wsl, /settings/connections/devices; remote routes /remote/:taskId, /remote/:conversationId and /remote-connections. [JS] SRC, high.
- Computer-use settings page: /settings/computer-use. [JS] SRC, high.
- Browser-use settings pages (other audit): /settings/browser-use/downloads, extensions, history, contact-info, passwords and site-settings; browser handoff route /browser/handoff. [JS] SRC, high (presence).

Memory and personalisation (taxonomy 8):
- Memories page: /memories route and memories settings. [JS] SRC, high.

Models and routing (taxonomy 9):
- Last-used model per user: /settings/user_last_used_model_config and /settings/user_tpp_last_used_model_config (the tpp expansion is unknown). [JS] SRC, medium.
- Usage settings page: /settings/usage. [JS] SRC, high.

Transparency and observability (taxonomy 10):
- Message feedback route: /conversation/message_feedback (feedback on individual messages). [JS] SRC, high (route), medium (UI).
- Telemetry endpoints: /ces/v1/log_event, /ces/v1/rgstr and /ces/statsc/flush (the CES expansion is unknown). [JS] SRC, high (routes), low (meaning).

Safety, permissions and governance (taxonomy 11):
- Account and security routes: /accounts/mfa_info, /accounts/verified_access, /accounts/optimized/check; age-status setting /settings/is_adult read from the settings API; /login; /codex-access. [JS] SRC, high (routes).
- Workspace and team routes: /workspace, /workspace-settings/web, /workspace/deactivated. [JS] SRC, high.
- OAuth callbacks: /connector/oauth_callback, /connector_platform_oauth_redirect and /chatgpt_health_oauth_redirect (a health-service OAuth redirect; meaning INF and not verified). [JS] SRC, medium.

UI and product design (taxonomy 12):
- Command palette: codex.command.* keys include newThread, newProjectlessTask, nextThread, previousThread, thread1 to thread9 (quick switch), searchChats, archiveThread, toggleThreadPin, findInThread, openFolder, openThreadInNewWindow, toggleSidebar, toggleTerminal, toggleBottomPanel, toggleReviewPanel, togglePinnedSummary, settings, showKeyboardShortcuts, showMiniOverlay, temporaryChat, logOut; plus commandMenu entries closeTab, closeWindow and copyConversationPath. [JS locale keys] SRC, high.
- Keyboard shortcuts settings page: /settings/keyboard-shortcuts. [JS] SRC, high.
- Browser sidebar context menu and load-error pages (back, forward, reload, inspect, copy link, open external browser, comment with Codex; DNS, proxy, firewall, certificate and timeout messages; zoom banner). Other audit. [JS locale keys] SRC, high.
- About dialog and in-app update confirmation: codex.aboutDialog.* and appHeader.installUpdate.* keys. [JS locale keys] SRC, high.
- Artifact feedback button: artifactFeedback.button.label. [JS locale keys] SRC, high.
- Settings pages (routes): /settings, /settings/account_user_setting, /settings/profile, /settings/user, /settings/personalization, /settings/usage, /settings/voices, /settings/keyboard-shortcuts, /settings/codex-micro (meaning unknown; low). [JS] SRC, high (routes).

Platform architecture (taxonomy 13):
- Mobile route: /codex-mobile appears in the renderer bundle; meaning INF (a mobile companion), low confidence.
- Chat service and realtime routes: /chat/frontend/v1/business-profiles and /chat/frontend/v1/business-profiles/presence; websocket user channel /celsius/ws/user (names not explained). [JS] SRC, low.

### 3B. App-server method names (string literals in resources/codex.exe)

Source: read-only grep -a of resources/codex.exe for domain/method patterns, then sorted and counted. The raw list (174 patterns, noisy because the binary concatenates neighbouring literals) is at scratchpad/tmp/codex_inv/rpc_methods_codex_exe.txt. Each name below matched a fixed-string search in codex.exe. A string match does not prove that the method is served, and no request or response schema was read. [bin] SRC, medium.

- thread: start, read, list, fork, resume, unsubscribe, archive, unarchive, delete, search, searchOccurrences, timeline/list, turns/list, items/list, settings/update, metadata/update, queue/add, queue/update, queue/reorder, compact/start, revert, goal/get, goal/set, backgroundTerminals/list, prediction/request, inject_items, increment_elicitation, decrement_elicitation, shellCommand, approveGuardianDeniedAction, attachment/add, attachment/list, attachment/remove, attachmentOwner/list. rollback is in the JS bundles only (section 3.1).
- thread realtime (thread-scoped): stop, appendText, outputAudio, transcript, item.
- turn: start, steer, interrupt, plan/updated (started and completed are in 3.4). review: start. command: exec. model: list.
- fs: readFile, writeFile, readDirectory, getMetadata, createDirectory, copy, remove, watch.
- config: read, value/write, batchWrite. externalAgentConfig: detect, import, import/recordHistory.
- account: read, login/start, logout, rateLimits/read, rateLimitResetCredit/consume, gatewayOAuth/read, gatewayOAuth/cancel, chatgptAuthTokens/refresh.
- memory: status, reset. feedback: upload. hooks: list. experimentalFeature: list. permissionProfile: list. project: create, read, update, move, delete, list.
- remoteControl: pairing/start, pairing/status, enable, disable, client/list. environment: add, info.
- plugin: list, read, install, installed, uninstall, search, reconcile, connector, skill/read, share/updateTargets, share/checkout. plugins: list, installed, cache, search, featured, suggested/codex, api_marketplace, marketplace, workspace. marketplace: add, upgrade.
- skills: list (with extraRoots), config/write, public, tree/main (GitHub skills-tree reference).
- mcpServer: mcpServerStatus/list, startupStatus/updated, reload, tool/call, resource/read, oauth/login, elicitation/request, event/stream.
- realtime (top level): start, listVoices, appendSpeech.

## 4. Extension points

- Plugins: .codex-plugin/plugin.json and .agents/plugins/marketplace.json; marketplace/add for further marketplaces. [SRC, bin]
- Skills: SKILL.md plus agents/openai.yaml; $-mentions; embedded system skills; remote skills. [SRC, bin]
- MCP servers: stdio and HTTP; per-plugin .mcp.json and config.toml mcp_servers; MCP Apps UI; OAuth login; elicitation. [SRC, bin]
- Hooks: the event list in 3.4 and handler types mcp_tool, command and agent_hook. [SRC, bin]
- App-server API with generated TypeScript and JSON Schema. [bin]
- CLI subcommands and flags: app-server, app-server-daemon, app-server-test-client, generate-ts, generate-json-schema, generate-internal-json-schema, --listen, --remote; "codex cloud" text. [bin]
- Config: config.toml (user and project level), profiles, model_providers, features, tui.keymap, notify, otel. [bin]
- Code mode (model-written JavaScript that calls tools) and the js REPL MCP tools. [bin, SRC]
- Connectors (named app integrations) and connector auth. [JS]
- Desktop IPC: internal, not an external surface. [JS]
- WebMCP policy for site-provided tools (browser plugin). [JS, DOC]
- Push and notification surfaces (service worker). [SRC]
- SDK-facing protocol types exist (app-server-types and app-server-protocol strings); no SDK package ships in this app bundle. Public webhooks not observed. INF, low.

## 5. Data and protocol hints

- Wire formats: JSON-RPC-style app-server methods (v2 names); MCP JSON-RPC; Electron IPC with chunking; Cap'n Web RPC; websocket (435 mentions in the binary, mostly transport and realtime code); TOML config; YAML skill interface metadata; SKILL.md front matter; remark directives in model output (::inbox-item); HTML fragments with a CDN allowlist (visualize); OOXML through WASM.
- Storage: better-sqlite3 (desktop app); SQLite tables in codex.exe; config.toml; CODEX_HOME folders (skills/.system, skills/remote-skills, plugins/cache, memories); Electron persisted atoms and thread read-state; managed worktree archives. Contents not read.
- Auth: ChatGPT account OAuth with PKCE; device-code login; refresh and id tokens; OS keyring; API-key login; gateway OAuth change notification. Strings only; no secrets read.
- Sync and multi-client: thread followers and thread streams; remote control enrolments with device keys; cloud environments (environment_id); Yjs dependency.
- Telemetry: OpenTelemetry trace headers; Sentry (DSN values not recorded); Statsig gates and evaluations (client identifiers not recorded).
- Trust: plugin receipts (RSA_PKCS1_SHA256, production key id); MSIX publisher identity.
- Hosts seen in data: chatgpt.com and chatgpt-staging.com (push worker allow-list); codex-portals.api.openai.com (masque-proxy-origins.txt). Other endpoint-like literals from REA are API or event names, not hosts.

## 6. Blockers and limits

- REA ran on .vite/build only (main process, 10.3 MB). The 561 MB renderer was not analysed by REA; it was keyword-scanned with Node. REA coverage is partial (semantic graph ceiling 100,000 nodes, many unknown facts). 36 of 38 IPC operations use dynamic channel names that REA did not resolve.
- Renderer English copy is mostly inside minified template literals, and the English locale catalogue was not located. UI feature names come from keys, not from visible copy.
- REA inspect_analysis_view with the sections collection fails for JS evidence (that view is for binary layouts only). Not retried.
- codex.exe was analysed by string grep only (no decompilation, no symbols beyond strings). Counts include noise from embedded libraries. Runtime version not determined. The Linux ELF (resources/codex) was not analysed.
- No workspace sources are shipped (monorepo packages are compiled into the bundles); no source maps; identifiers are minified.
- Not read, by rule: per-user stores under AppData (Codex, Claude, Cursor, t3code, Antigravity, Aside, OpenAI), cookies, login data, IndexedDB, keyring, auth files, the user .codex config and memories, and any user AGENTS.md. No secret-bearing file was opened; client identifiers (Sentry, Statsig) were not recorded.
- Not inspected: resources/accessibility/, cua_node internals beyond the manifest, tectonic.exe, rg.exe (not run), codex-windows-sandbox-setup.exe internals, default_app, and THIRD_PARTY_NOTICES beyond its header (it lists JavaScript packages only).
- Shell hygiene: one of my own grep jobs stalled past the shell timeout; I stopped my own stray grep processes. The sibling file hermes.inventory.md in the same findings folder was not touched.
- Confidence is medium or low for the model slugs and for tool schemas (names only, no schemas read).
- Continuation pass: a recursive grep over the 561 MB renderer and a du over the 594 MB extract both timed out in the shell and were moved to the background. Their results were either not needed or re-run on the bundle files only.
- Continuation pass: the REA summary of the .vite/build analysis was retrieved, but its persisted copy (saved by the harness in the Claude Code session store) was not opened. The IPC statistics in section 3.13 come from the earlier pass and were not re-read.

## 7. REA usage log (tools actually called)

1. analyze_javascript_application on .vite/build (format directory, detail summary, integrity_policy record-and-continue, max_heap_mb 2048, analysis_timeout_ms 900000). OK. Parent evidence ev_4300ff8e3aa5ef90db8da1410258d0039845fd07911aabab35f6fe078bc228b4; summary evidence ev_036666b8c851db345584cd734d583f4efb45a77e826b0492ba74b9c6d3aefad7. Statistics: 34 files, 10,333,165 bytes read, 2,700,745 AST nodes, 1,079 findings, 0 parse failures, 0 modules recognised.
2. inspect_analysis_view (sections page on the parent evidence). Error: that view is only valid for binary-layout evidence. Not retried.
3. trace_application_feature, seed string "subagent" (contains, case-insensitive), direction both. OK: 1 seed match, 385 nodes, 566 edges, 66 terminal paths, 0 native handoffs.
4. trace_application_feature, seed string "plugin" (contains, case-insensitive), direction both. OK: 2 seed matches on the same graph size. Endpoint literals and IPC nodes were extracted from this trace (JS asset nodes, 13 endpoint nodes, 12 IPC channel nodes, 2 IPC handlers, 1 worker created from a base64 data URL).
5. inspect_pe_resources: Codex.exe (OK; 20 KB; RT_MANIFEST only). ChatGPT.exe (OK; 59 resources, 6 icon groups; output saved by the harness and parsed with Node). resources/codex.exe with max_file_bytes 512 MiB (OK; no resource directory).
6. Continuation pass: inspect_analysis_view, view summary, source retained-evidence ev_4300ff8e3aa5ef90db8da1410258d0039845fd07911aabab35f6fe078bc228b4 (the parent of item 1). OK. The result exceeded the inline limit and was saved by the harness. Its 2 KB preview matched item 1 (34 files, 10,333,165 bytes, 2,700,745 AST nodes, 1,079 findings, 0 parse failures, 0 modules). The saved copy was not opened.
7. Continuation pass: the first call used summary evidence ev_036666b8c851db345584cd734d583f4efb45a77e826b0492ba74b9c6d3aefad7 as the source. It was refused as an unsupported target, because that evidence records an inspect_analysis_view operation rather than the analysis itself. The parent in item 6 worked.
Not used: inspect_artifact (would inventory the 1.7 GB resources tree), binary_overview, search_strings, list_strings and open_binary (shared active target; read-only grep was used for the binaries instead), analyze_function, and the native trace tools (no native target was opened).

## 8. Suggested deep-dive targets (highest value first)

1. codex.exe agent runtime: app-server v2 protocol (thread, turn, item, plugin, skills, config, fs, account, realtime); built-in tool registry and schemas; subagent lifecycle (spawn, wait, send, resume, close; max_depth); compaction; goals; memory pipeline (memory_* identifiers, memories config, CLAUDE.md import path).
2. code_mode: codex-code-mode-host.exe (V8) and the code_mode tool contract; the js REPL MCP servers node_repl and cua_repl.
3. Sandbox and exec: sandbox_mode and network_access; exec policy (.rules, prefix_rule); Windows sandbox users, WFP rules, command-runner job-object and token setup; approval policies and the approvalsReviewer and guardian path.
4. Plugin and skill lifecycle in main-BklS_2Y2.js: marketplace loading; plugin/install; signature verification (receipt.json); skill discovery and $-mention resolution; hook dispatch (mcp_tool, command, agent_hook).
5. Automations and inbox: automations.local gate, automation_runs, the ::inbox-item directive contract, cloud (aeon) threads and environments.
6. Model capability handling: model/list entries; reasoning effort and service-tier wiring; provider routing (model_providers, wire_api, oss providers); image and realtime model use; web_search modes.
7. Pets (hatch-pet SKILL.md and scripts), Appshot and dictation hotkeys, avatar overlay: candidates for Verevon's personality and companion features.
8. Visualize skill (HTML fragment contract, CDN allowlist, render.py) as a reference for in-chat generative UI.
9. Deep-research plugin contract (research worker, report-source.md, DOCX via $documents) as a reference for research mode.
10. MCP Apps sandbox (host and guest messages, init_rejected path) and the code-review MCP tool surface.

Cross-references for the other audits: in-app browser (plugins/browser, renderer browser-* keys, CDP usage), Chrome plugin, Computer Use plugin, and the sibling hermes.inventory.md.
