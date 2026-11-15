# Cursor IDE 3.17.21: inventory (AUDIT 3 scout, t3)

Role: inventory scout for the later deep-dive agent (Verevon Model Plane and chat/agent pages). Breadth and accurate pointers matter more than depth. Code is ground truth; docs show intent.

Read-only audit: nothing under the install directory or the user data directories was written or read. No app, installer, dev server or build was run.

Paths use forward slashes.
- INSTALL = C:/Users/ImaFernandesDaCosta/AppData/Local/Programs/cursor
- APP = INSTALL/resources/app
- OUTCOPY = scratchpad/tmp/cursor/app/out (byte copy of APP/out, used as the REA input and for greps)
- Scratch helpers and raw dumps: scratchpad/tmp/cursor/ (manifests.json, flags_client.txt, ctx.js, kw.js, kw2.js, rea_*.js)

Provenance tags: REA = observed through REA analysis of a shipped artifact. SRC = read directly in shipped files (bundle, package.json, product.json, extension manifests, dist code). DOC = documentation; none used (no docs in the install, no web fetch in this scout). INF = inference, with the basis stated. Most bundle facts are SRC on minified code: names and strings are reliable, behavior is inferred.

---

## 0. Method and REA usage log

Install shape: directory-based, no app.asar (node_modules.asar is a 28-byte placeholder; node_modules is unpacked). SRC.
No pre-extracted Cursor copy exists under scratchpad/extract/ (that set holds antigravity, claude, codex, grokbot, manus, slack, t3code). The scout copied APP/out (121 MB, 173 files), APP/package.json and APP/product.json into scratch and ran REA against the copy only.

REA tools actually used:
1. analyze_javascript_application (format=directory, detail=summary, integrity_policy=record-and-continue, max_heap_mb=2048, analysis_timeout_ms=1200000) on OUTCOPY. Result: 57 relevant files; 48 parsed JS files; 2 parse failures; 2,951 findings; Electron: 6 browser windows, 4 explicit web preferences, 4 context-bridge APIs, 18 IPC operations (5 literal channels, 12 dynamic, 3 main handlers, 3 renderer listeners, 12 renderer transmissions, 3 unpaired literal transmissions), 1 utility process entry, 0 native addon bindings. Coverage partial (semantic layer truncated; 49 application unknowns, 2,514 semantic unknowns). Session evidence id ev_e36e1eedc725e9df78ee44d90a7f13cdf3ba1e1ce9c07c23da21c91423856fa2 (retained in session only).
2. inspect_analysis_view on that evidence: collection "sections" rejected (binary-layout only); collection "modules" (offset 0, limit 48) returned 48 nodes, listed in section 2.
3. trace_application_feature, seed kind=channel with empty exact value: status no-match. Empty seeds do not enumerate channels, so channel names come from grep instead.
4. inspect_pe_resources on INSTALL/Cursor.exe (max_file_bytes 268435456): PE32+ x64, 214,092,584 bytes, sha256 92bb8ca2d68ea131663b0abbf0ac5349beb2bafbf750ecd44e6aa83d520a6125; 53 resources; RT_VERSION (en-US, 788 bytes) and RT_MANIFEST (en-US, 1425 bytes) present; 6 icon sizes. Payloads other than RT_GROUP_ICON stay opaque (REA limit). REA
Not used: open_binary, binary_session, binary_overview, search_strings, list_strings (they need the shared active target; avoided per brief).

Other method: Git-Bash grep (-a -o -h) over OUTCOPY and APP/extensions; Node read-only JSON parsing (product.json, package.json, 116 extension manifests) and regex keyword scans (writes only to scratch).

---

## 1. Identity

- Product: Cursor (nameShort/nameLong "Cursor"); author "Anysphere, Inc." (APP/package.json). SRC.
- Version: 3.17.21 (APP/package.json, APP/product.json); quality "stable"; product commit 8f2a112cb2845a97b75fd932ea5c470579ca4060; build date 2026-08-25T01:05:08Z (APP/product.json). SRC.
- Base: VS Code OSS fork; product.json vscodeVersion 1.128.0. SRC.
- Runtime: Electron 40.10.3; Chromium 144.0.7559.236 (version strings in INSTALL/Cursor.exe). SRC (binary strings).
- Identity keys (APP/product.json): applicationName "cursor"; dataFolderName ".cursor"; urlProtocol "cursor"; serverApplicationName "cursor-server"; serverDataFolderName ".cursor-server"; tunnelApplicationName "cursor-tunnel"; win32AppUserModelId "Anysphere.Cursor"; updateUrl https://api2.cursor.sh/updates; backupUpdateUrl http://cursorapi.com/updates. SRC.
- Sizes: out/ 121 MB; extensions/ 151 MB; node_modules size NOT measured. SRC.
- Install layout (INSTALL): Cursor.exe (214 MB); resources/app/bin/{cursor, cursor.cmd, cursor-tunnel.exe, code-tunnel.exe}; resources/app/codeBin/{code, code.cmd}; tools/inno_updater.exe; unins000.exe (Inno Setup); locales/*.pak (about 50 Chromium locales); resources.pak; icudtl.dat; ffmpeg.dll; vk_swiftshader; d3dcompiler_47; dxcompiler; dxil. SRC.

Tech stack:
- TypeScript/JavaScript, bundled and minified: out/main.js (2.47 MB), out/cli.js, out/vs/workbench/workbench.desktop.main.js, out/vs/workbench/workbench.glass.main.js, out/vs/workbench/api/node/extensionHostProcess.js. SRC.
- React runtime: out/vs/workbench/react-runtime/react-dom/esm-index-production.js. SRC.
- Protobuf plus Connect-RPC service descriptors (aiserver.v1, agent.v1). SRC.
- Sentry (@sentry/electron), OpenTelemetry, Statsig SDK. SRC.
- SQLite via @vscode/sqlite3 (state.vscdb). SRC.
- Native modules: node-pty, keytar, native-keymap, kerberos, native-watchdog, native-is-elevated, node-ovsx-sign, @parcel/watcher, @vscode/ripgrep, @vscode/spdlog, @vscode/sqlite3, @vscode/windows-registry, @vscode/windows-process-tree, @vscode/windows-mutex, @vscode/windows-ca-certs, @vscode/deviceid, @vscode/sudo-prompt, @vscode/tree-sitter-wasm. SRC.

Dependency list grouped by capability (APP/node_modules top-level, about 330 packages; full list via `ls APP/node_modules`). SRC. Extension manifests declare no npm deps (bundled into dist). Extensions/node_modules holds only esbuild-wasm and typescript.
- Agent/AI transport and schema: @connectrpc/connect, @connectrpc/connect-node, @bufbuild/protobuf, @modelcontextprotocol/sdk, zod, zod-to-json-schema, @anysphere/policy-watcher, @typespec/ts-http-runtime, eventsource, eventsource-parser, @c4312/eventsource-umd, undici, node-fetch, ws, hono, @hono/node-server, express, @fastify/busboy, cors, send, serve-static, seroval, seroval-plugins, pkce-challenge.
- Auth/identity: @azure/identity, @azure/msal-node, @azure/msal-browser, @azure/msal-common, @azure/core-auth, @azure/core-client, @azure/core-rest-pipeline, @azure/core-util, @azure/logger, jose, jsonwebtoken, jws, jwa, fast-jwt, keytar, native-is-elevated.
- Markdown, parsing and text: gray-matter and section-matter (frontmatter for .mdc rules and skills), js-yaml, yaml, json5, jsonc-parser, ajv, json-schema-traverse, parse5 (+adapters), cheerio, htmlparser2, css-select, css-what, domutils, dom-serializer, entities, xml2js, xmlbuilder, sax, esprima, acorn, @babel/* (parser, generator, traverse, types, template), vscode-textmate, vscode-oniguruma, vscode-regexpp, @vscode/vscode-languagedetection, @vscode/tree-sitter-wasm, @chenglou/pretext (purpose INF), lodash, underscore.
- Search, retrieval and files: @parcel/watcher, minimatch, picomatch, path-scurry, readdir-glob, fs-extra, graceful-fs, rimraf, mkdirp, tmp, which, cross-spawn, shebang-command, open, foreground-child, archiver, archiver-utils, compress-commons, crc-32, crc32-stream, zip-stream, yazl, yauzl, unzipper, tar, tar-stream, minizlib, file-type, peek-readable, strtok3, token-types, @tokenizer/token.
- Terminal, processes and sandboxing: node-pty, @xterm/xterm, @xterm/headless, @xterm/addon-{clipboard, image, ligatures, progress, search, serialize, unicode11, webgl}, pidusage, cursor-proclist, native-watchdog, run-applescript, default-browser, is-wsl, is-docker, is-inside-container, wsl-utils.
- Browser automation and remote desktop: chrome-remote-interface (CDP client), @novnc/novnc, react-vnc (VNC viewer; use not traced, INF on purpose).
- UI, state and motion: react, react-dom, solid-js (present; use not verified), jotai, framer-motion, motion, motion-dom, motion-utils, @dnd-kit/{accessibility, core, sortable, utilities}, @tanstack/{query-core, react-query, solid-query, react-virtual, virtual-core}, rxjs, scheduler, tinycolor2.
- Images and documents: @jimp/{core, file-ops, js-bmp, js-gif, js-jpeg, js-png, js-tiff, plugin-resize, types, utils}, gifwrap, omggif, pngjs, jpeg-js, utif2, image-q, exif-parser, opentype.js. PDF rendering uses pdf.js shipped under out/vs/glass/browser/media (not in node_modules).
- Observability and tracing: @sentry/{browser, core, electron, node, node-core, opentelemetry, types}, @sentry-internal/*, @opentelemetry/{api, api-logs, core, resources, sdk-logs, sdk-metrics, sdk-trace-base, sdk-trace-node, exporter-trace-otlp-proto, otlp-exporter-base, otlp-transformer, semantic-conventions, context-async-hooks} plus many @opentelemetry/instrumentation-* packages (http, undici, express, fs, pg, redis, ioredis, mysql, mysql2, mongodb, mongoose, kafkajs, knex, koa, hapi, graphql, connect, dataloader, generic-pool, lru-memoizer, tedious, amqplib), @prisma/instrumentation, @apm-js-collab/{code-transformer, tracing-hooks}, import-in-the-middle, require-in-the-middle, shimmer.
- Database clients (present, use not traced): pg, pg-protocol, pg-types, pg-int8, postgres-array, postgres-bytea, postgres-date, postgres-interval (INF: likely pulled in by OTel instrumentation).
- Networking and proxies: @tootallnate/once, agent-base, http-proxy-agent, https-proxy-agent, socks, socks-proxy-agent, tunnel, tunnel-agent, proxy-from-env, typed-rest-client, azure-devops-node-api (Azure DevOps client), cookie, tough-cookie, whatwg-url, encoding, iconv-lite, jschardet.
- Build-time and misc: caniuse-lite, browserslist, electron-to-chromium, update-browserslist-db, @types/*, @typespec/*.

---

## 2. Directory and module map

INSTALL and APP (SRC):
- Cursor.exe: Electron shell (PE32+ x64).
- resources/app/bin: cursor (CLI launcher), cursor.cmd, cursor-tunnel.exe, code-tunnel.exe (tunnels).
- resources/app/codeBin: code, code.cmd (VS Code-style launcher).
- resources/completions/{bash, zsh}: shell completions.
- resources/app/package.json: name Cursor, version 3.17.21, main ./out/main.js, type module.
- resources/app/product.json: identity, marketplace and telemetry config; statsigClientKey present (value deliberately not recorded).
- resources/app/replaceCode.bat, restoreCode.bat: install-side patch scripts (not analyzed).

out/ (121 MB, 173 files):
- out/main.js (2.47 MB): Electron main process (app lifecycle, Sentry, Statsig gating, auth, update, deep links, extension signing map).
- out/cli.js (226 KB): CLI argument parser (VS Code-style options; option names include agent, cloud, sandbox, agentCompatService).
- out/bootstrap-fork.js: child process bootstrap.
- out/nls.keys.json, out/nls.messages.json: VS Code localized strings.
- out/media/ (31 files): icons and logos.
- out/vscode-dts/vscode.d.ts: VS Code extension API typings.
- out/vs/workbench/workbench.desktop.main.js: main IDE renderer (editor, Composer/agent UI, service layer).
- out/vs/workbench/workbench.glass.main.js: "Glass" renderer, the agent-first window (cloud projects, diff tabs, side chats).
- out/vs/workbench/workbench.anysphere-ui-automations.js: Automations UI (webhook panel, run mode, Mermaid component).
- out/vs/workbench/api/node/extensionHostProcess.js: extension host (Node); holds aiserver.v1 client descriptors and hook event names.
- out/vs/workbench/api/worker/extensionHostWorkerMain.js: web-worker extension host.
- out/vs/workbench/react-runtime/: React and ReactDOM production runtime.
- out/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateTokenizationWorker.workerMain.js: TextMate tokenization worker.
- out/vs/workbench/contrib/codeEditor/browser/dictation/: editor dictation (nls keys reference it).
- out/vs/code/electron-main/: main-side services (not enumerated in this scout).
- out/vs/code/electron-sandbox/: workbench.js, extensionMonitor/extensionMonitor.js, processExplorer.js.
- out/vs/code/electron-utility/: conversationSearch/conversationSearchMain.js (conversation search index), mcpProcess/mcpProcessMain.js (MCP runtime), sharedProcess/sharedProcessMain.js (shared process).
- out/vs/glass/browser/effects/effectsWorkerMain.js: Glass visual-effects worker.
- out/vs/glass/browser/media/: pdf.min.mjs, pdf.worker.min.mjs (pdf.js), cursor-splash-logo-{glass, normal}.png, cloud-environment-setup.png, claude-code-import-claude-mark*.png.
- out/vs/platform/browserView/electron-main/preload-browser.js and out/vs/platform/browserView/electron-sandbox/externalSchemeConfirmation: in-app browser preload and external-scheme confirmation.
- out/vs/platform/files/node/watcher/watcherMain.js: file watcher process.
- out/vs/platform/tracing/browser/media/sentryReplayCompressionWorker.js: Sentry replay worker.
- out/vs/platform/{terminal, profiling, update, accessibilitySignal}: VS Code platform services.
- out/vs/base/parts/sandbox/electron-sandbox/preload-aux.js: sandbox preload.

extensions/ (116 entries = 115 extensions + node_modules):
- 20 cursor-* extensions (see section 4 list); 12 theme-* (theme-cursor, theme-abyss, theme-defaults, theme-kimbie-dark, theme-monokai, theme-monokai-dimmed, theme-quietlight, theme-red, theme-seti, theme-solarized-dark, theme-solarized-light, theme-tomorrow-night-blue).
- About 80 VS Code built-ins: language support (typescript, javascript, json, css, html, markdown, python, cpp, csharp, go, rust, java, php, ruby, swift and others), git and github (git, git-base, github, github-authentication, microsoft-authentication), debug (ms-vscode.js-debug, js-debug-companion, debug-auto-launch, debug-server-ready), notebooks (ipynb, notebook-renderers, vscode-jupyter-keymap), media-preview, simple-browser, terminal-suggest, tunnel-forwarding, npm, grunt, gulp, jake, docker.
- node_modules/: esbuild-wasm, typescript.

REA module nodes (from inspect_analysis_view "modules", 48 items; REA):
- out/main.js; out/vscode-dts/vscode.d.ts (repeated); out/vs/glass/browser/media/pdf.min.mjs; out/vs/glass/browser/media/pdf.worker.min.mjs (listed as asset/module); out/vs/platform/tracing/browser/media/sentryReplayCompressionWorker.js; out/vs/workbench/services/textMate/...textMateTokenizationWorker.workerMain.js; out/vs/workbench/react-runtime/react-dom/esm-index-production.js; out/vs/workbench/api/node/extensionHostProcess.js; out/vs/code/electron-sandbox/extensionMonitor/extensionMonitor.js; out/vs/code/electron-utility/conversationSearch/conversationSearchMain.js; out/vs/code/electron-utility/mcpProcess/mcpProcessMain.js; out/vs/platform/browserView/electron-main/preload-browser.js; out/vs/base/parts/sandbox/electron-sandbox/preload-aux.js; out/vs/platform/files/node/watcher/watcherMain.js; out/vs/code/electron-sandbox/workbench/workbench.js; out/vs/workbench/workbench.anysphere-ui-automations.js.

Processes (REA + SRC):
- Main: out/main.js. Renderers: workbench.desktop.main.js (IDE), workbench.glass.main.js (Glass), workbench.anysphere-ui-automations.js.
- Utility processes: conversationSearch, mcpProcess, sharedProcess, extensionMonitor, processExplorer, watcher, plus Sentry-replay, Glass-effects, TextMate and PDF workers.
- Extension host: extensionHostProcess.js (Node) and extensionHostWorkerMain.js (web).
- Agent host: cursor-agent-host (hosted in the AgentExec extension host); cursor-local-agent-runtime (described as hosting "Private Inference outside workspace extension hosts"). Exact process model INF.
- PTY and tmux: agent.v1.PtyHostService and agent.v1.TmuxSessionService (names only).

---

## 3. Feature candidates (grouped by taxonomy)

Each bullet: capability, pointer, tag, confidence.

### 1. Chat experience
- Mode and toggle labels in the chat UI: Agent, Ask, Plan, Debug, Multitask, Zen, Max, Voice, Design, Run, Privacy, Compatibility modes. Pointer: label strings in out/vs/workbench/workbench.desktop.main.js and workbench.glass.main.js. SRC. High for labels; medium for semantics.
- Mermaid diagram rendering in chat markdown. Pointer: "Mermaid" component in out/vs/workbench/workbench.anysphere-ui-automations.js (also desktop and glass bundles). SRC. High.
- Markdown rendering built on marked and hast utilities (vendor strings). Pointer: desktop and glass bundles. SRC. Medium.
- Conversation search, both as a tool and as an index process. Pointers: out/vs/code/electron-utility/conversationSearch/conversationSearchMain.js; tool search_conversations; ConversationsService (aiserver.v1). SRC/REA. High.
- Conversation organization: sidebar nesting, grouping, recency filter, archive, chat tabs, split tabs, pinned skills. Pointers: glass_sidebar_* and glass_chat_tabs / glass_split_tabs flags (flag list: scratch tmp/cursor/flags_client.txt). SRC. Medium.
- Shareable agent links: aiserver.v1 CreateAgentShare (share_id, share_url), GetAgentSharePreview (include_preview_image). Pointer: out/main.js protobuf descriptors; flag cloud_glass_shared_sessions. SRC. High.
- Shared session file watcher: cursor_shared_session_file_watcher flag. SRC. Medium.
- Notepads (reusable context notes attached to messages): aiserver.v1.ConversationMessage.NotepadContext and "notepads" field. Pointer: out/main.js. SRC. High.
- Conversation table of contents, end-of-turn summary, working indicator, typed header table: composer_* and conversation_table_of_contents flags. SRC. Medium.
- Message actions: fork from turn (glass_cloud_agent_fork_from_turn), resubmit from message (cloud_agent_resubmit_from_message), queued prompts (agent_queued_prompt_new_request_id), follow-up prompt editor (glass_retain_followup_prompt_editor). SRC flags. Medium.
- Canvas: browser_canvas and context_usage_canvas flags only; a "Canvas" feature was not confirmed (HTML canvas noise). SRC flags; purpose INF. Low.
- Context visualizer and context usage display: context_visualizer and context_usage_canvas flags; "Context window" label. SRC. Medium.
- Projects as workspaces: glass_projects_enabled, glass_projects_as_agents, glass_project_* flags; enable_project_layouts_in_system_prompt. SRC flags. Medium.
- Streaming transport: Connect streaming with decode/encode yields (ai_connect_stream_decode_yield, ai_connect_stream_encode_yield); HTTP/2 with SSE fallback (disableHttp1SSE). SRC. High.
- Inline assist: Cmd+K inline edit (aiserver.v1 CmdKService); terminal Cmd+K (terminal.cmdk, terminal.cmdk.submit). SRC. High.

### 2. Multimodal and creation
- Image generation tool: generate_image (client ToolCall map: generateImageToolCall). Pointer: out/vs/workbench/workbench.desktop.main.js. SRC. High for tool presence; medium for behavior.
- Image attachments in CLI: cli_image_arg, cli_show_images flags. SRC flags. Medium.
- Voice mode: "Toggle Voice Mode" command; "Voice Mode" label; voice input flags. Pointer: desktop bundle. SRC. Medium.
- Editor dictation (speech to text in the editor): out/vs/workbench/contrib/codeEditor/browser/dictation/ (nls keys reference it). SRC. Medium.
- Video understanding: gemini_video_developer_api, enable_watch_video_in_ide, agent_video_signed_url_uploads, glass_video_player_parity flags. SRC flags. Medium.
- PDF viewer and file tab preview: out/vs/glass/browser/media/pdf.min.mjs and pdf.worker.min.mjs; glass_pdf_file_tab_preview flag. SRC. High.
- Notebook support: ipynb, notebook-renderers, vscode-jupyter-keymap extensions. SRC. High.
- In-app image codecs (jimp, pngjs, jpeg-js, gifwrap, utif2, image-q): node_modules. SRC. Medium.
- Document, slide and spreadsheet generation: no dedicated tool or library found (only glass_rainbow_csv_extension flag). INF from absence. Low.
- Data analysis and charting: no chart library or tool name found. INF from absence. Low.

### 3. Research and web
- Web search tool: web_search (client ToolCall map: webSearch). SRC. High.
- Web fetch tool: web_fetch (ToolCall map webFetch; tool-kind enum includes WEB_FETCH). SRC. High.
- Deep research: no "Deep research" label or flag found. INF from absence. Medium.
- Browser reading and searching through the in-app browser and MCP browser tools (see section 6). SRC. High.
- Connectors and integrations: connect_scm tool; connectSourceControlFromInAppAd command; Vercel connect pill flags (glass_vercel_connect_pill_cloud, glass_vercel_connect_pill_local); Slack flags (cloud_agent_slack_default_worker, cloud_agent_slack_chatter_muted_rows). SRC. Medium.
- Docs context: "@Docs" label not found; disable_docs_client_usage flag present. SRC. Medium.
- Source-control providers: GitHub API (api.github.com graphql and search/issues), GitLab, Bitbucket Server, Azure DevOps (azure-devops-node-api), github.com app install link. SRC. High.

### 4. Agent runtime
- Agent loop with tool calling: about 60 mapped tool names (section 5). SRC. High.
- Planning: create_plan (createPlanToolCall); plan mode and build in cloud (plan_mode_build_in_cloud flag); .cursor/plans/ directory. SRC. High.
- To-do tracking: todo_write (updateTodosToolCall), todo_read (readTodos). SRC. High.
- Subagents: task_v2 (taskToolCall); hooks subagentStart and subagentStop; flags explore_subagent, explicit_subagent_models, enable_await_for_subagents, cursor_agent_host_native_background_cloud_subagent_creation; .cursor/agents/ directory. SRC. High.
- Browser subagent: browser_subagent flag; sand_browser_use_subagent flag. SRC flags. Medium.
- Parallel attempts (best-of-N): cloud_agent_best_of_n_disabled flag; best_of_n_group_id and try_use_best_of_n_pr protobuf fields. SRC. High.
- Background and cloud agents: aiserver.v1 BackgroundComposerService; background_composer (about 558 occurrences); tools create_agent, get_agent_status, stop_agent, send_to_agent, read_agent_transcript, fetch_cloud_agent_data; cloud_environment_template; cloud_agent_* flags (34); .cursor/environment.json; cursor.backgroundcomposer command. SRC. High.
- Cloud VM environments: aiserver.v1 VmDaemonService; tool setup_vm_environment; cloud-environment-setup.png; cloud_agent_env_setup_with_dockerfiles flag. SRC. High.
- Long-running jobs: long_running_jobs flag; grind mode with tools start_grind_planning and start_grind_execution (UI labels Planning and Executing); enable_grind_mode flag. SRC. Medium.
- Goals: create_goal and update_goal tools; agent_goal_continuation flag. SRC. Medium.
- Automations (scheduled and webhook-triggered agents): aiserver.v1 AutomationsService and SchedulerService; automations_* flags (retry, chain prompts, org-scoped triggers, validation); automation webhook panel in out/vs/workbench/workbench.anysphere-ui-automations.js ("webhook payload is forwarded to the automation run context"); deeplink cursor://anysphere.cursor-deeplink/automations. SRC. High.
- Automation memories: listAutomationMemories RPC; "Memories" enum label. SRC. Medium.
- Babysit PR (cloud): babysit_pr_in_cloud_params protobuf field; expose_babysit_pr_cloud_entrypoint flag. SRC. Medium.
- Full self-driving (team-level autonomy): aiserver.v1 FullSelfDrivingService; team.full_self_driving.manage permission string; full_self_driving_glass_pr_tab flag. Purpose unverified. SRC names; INF on behavior. Low.
- Checkpoints and rewind: checkpointAt column in the composer schema; restoreWorkspaceFiles; "<checkpoint-revert>" prompt tag; composer_lazy_inline_checkpoints flag; "Hydrating repo files from remote checkpoint". SRC. High.
- Error recovery: nal_agent_retries flag; glass_auto_continue_interrupted_agents and glass_auto_continue_interrupted_subagents flags; ask_question_durable_delivery flag. SRC. Medium.
- Hook points in the agent lifecycle: 21 events (section 4). SRC. High.
- Evals and replay: aiserver.v1 EvalTrackingService, RequestReplayService, ReplayChatService, ChatRequestEventV2Service, ToolCallEventService. SRC names. High for presence; low for use.
- Asking the user: ask_question tool (askQuestionToolCall); ask_question_all_modes flag; cloud_agent_ask_question_auto_answer flag. SRC. High.
- Mode switching: switch_mode tool (switchModeToolCall). SRC. High.
- Reflection: reflect tool. SRC. Medium.
- Progress reporting: update_current_step (communicateUpdate), send_to_user, send_final_summary. SRC. High.
- Shell execution: run_terminal_command_v2 (shell), write_shell_stdin; run_terminal_cmd also appears in older definitions. SRC. High.
- Remote execution authority: cursor-resolver (remote authority background-composer), cursor-resolver-helper, cursor-socket. SRC. Medium.
- Grok Bot: aiserver.v1 GrokBotService; grok_bot_conversation_gc, grok_bot_dynamic_tools, grok_bot_shell_dedupe flags; sand_get_grok_bot_ios flag. Relation to a separate Grok Bot app not verified. SRC. Medium.
- Context compaction and KV cleanup: agent_kv_auto_gc and composer_gc_handles flags; preCompact hook. Behavior unverified. SRC names; INF. Low.
- Auto-routing and queued follow-ups: auto_background_foreground_tools_on_followup flag; composer_auto_routing_result_display flag. SRC. Medium.

### 5. Tools, plugins and extensibility
- MCP client: cursor-mcp extension (handles MCP) and mcpProcessMain (utility process). Transports: stdio (33 hits), streamableHttp (38), sse (23). OAuth flows with oauth-attempt state in globalState and secrets (171 hits). Pointers: cursor-mcp/dist; out/vs/code/electron-utility/mcpProcess/mcpProcessMain.js. SRC. High.
- MCP configuration: mcp.json (user and project), .cursor/mcp.json, mcp.deeplinkInstall setting, mcp.ipc.count. SRC. High.
- MCP admin controls: mcp_admin_only_servers, mcp_admin_only_tools, mcp_allowlists, mcp_network_allowlist, mcp_access_network_allowlist; allowedMcpConfiguration (admin); admin-mcp-tool-allowlist.ts module. SRC. High.
- MCP tool surface: get_mcp_tools, fetch_mcp_resource (readMcpResourceToolCall), list_mcp_resources, mcp_auth. Meta-tool slim descriptors flag (mcp_meta_tool_slim_descriptors). SRC. High.
- MCP registry and marketplace: aiserver.v1 MCPRegistryService and MarketplaceService; marketplace.cursorapi.com (galleryId "cursor"); marketplaces_enabled and marketplace_origin_distribution flags. SRC. High.
- Plugins: .cursor/plugins/ and .cursor/plugins/cache; plugin_id (148 occurrences); enable_local_3p_plugin_imports, import_3p_plugins, enable_cc_plugin_import flags; cloud_agent_plugins flag. SRC. High.
- Skills: .cursor/skills/, .cursor/cloud-skills/, .cursor/skills-cursor/; cursor_skill tool kind; skillStore (move in and out of store); publishSkill (pack as plugin); unpublishSkill; resyncSkill; cursor_skill_enabled flag. SRC. High.
- Rules: .cursor/rules/ directory; .mdc files with frontmatter (alwaysApply; mdc.js module in workbench); legacy .cursorrules; AGENTS.md and CLAUDE.md read as instruction files. SRC. High.
- Commands: .cursor/commands/ (named in mcp ignore patterns); cursor_command field. SRC. Medium.
- Hooks: hooks.json (14 hits); 21 event names (section 4); customize_hooks_execution_log_detail flag. SRC. High.
- Custom subagents and modes: .cursor/agents/ (4 hits); custom_agents_enabled, cloud_custom_modes, glass_custom_modes flags. SRC. Medium.
- Customize page: customize_page, customize_plugin_detail_mcp_management, customize_manage_scope_multiselect flags. SRC. Medium.
- Deep links: cursor:// URL protocol; cursor-deeplink extension; trustedExtensionProtocolHandlers includes anysphere.cursor-deeplink and anysphere.cursor-mcp. SRC. High.
- CLI: resources/app/bin/cursor launcher; out/cli.js options (agent, cloud, sandbox); cli_* flags (auto_run_hint, btw_side_question, debug_mode, harness_telemetry, image_arg, instant_grep_indexing, model_picker, sandbox_default_enable, show_images, zen_mode_available); cursor.installCli command. The standalone agent CLI binary is not in this install. SRC. Medium.
- Desktop bridge (CLI to desktop app): cursor.desktopBridge.enabled setting; desktopBridgeCliClient.js (in cli.js and main.js). SRC. Medium.
- Extension API: 54 distinct enabledApiProposals across extensions; out/vscode-dts/vscode.d.ts. SRC. High.
- Package names referenced: @cursor/types, @cursor/experiment-config, @cursor/build-flags (in bundle; usage unclear). SRC. Low.

### 6. Computer and environment control
- In-app browser (Electron BrowserView): preload-browser.js; cursor.browserView.* commands (executeJavaScript, listTabs, getURL, takeScreenshot, newTab, newHeadlessTab, navigate, reload, closeTab, selectTab, hideTab, setLocked, isLocked, sendCDPCommand, updateScreenshot, getTitle); browser_features_access_control flag. SRC. High.
- Browser origin allowlist: cursor.browserOriginAllowlist.ensurePageOriginAllowed and ensureNavigationAllowed; browserOriginAllowlist admin setting. SRC. High.
- Browser automation MCP server (cursor-browser-automation, publisher cursor): 19 tools: browser_snapshot, browser_navigate, browser_click, browser_type, browser_fill, browser_fill_form, browser_select_option, browser_hover, browser_drag, browser_press_key, browser_scroll, browser_mouse_click_xy, browser_take_screenshot, browser_get_bounding_box, browser_highlight, browser_tabs, browser_lock, browser_cdp, browser_cpp_telemetry. SRC. High.
- browser_navigate validation: rejects invalid URLs and checks the origin allowlist. SRC. High.
- Computer use: computer_use (protobuf oneof, ToolCall computerUse); record_screen tool; sand_computer_use_playwright and sand_computer_use_unicode_typing flags. SRC. Medium.
- Teach by demonstration: sand_teach_by_demonstration flag only. SRC. Low.
- Remote desktop: @novnc/novnc and react-vnc present (use not traced). SRC presence; INF on purpose. Low.
- Terminal: node-pty, @xterm/* addons, PtyHostService and TmuxSessionService (agent.v1); terminal.integrated.* settings (VS Code); cursor.terminal.enableAiChecks and cursor.terminal.usePreviewBox commands. SRC. High.
- Sandboxed execution: cursor-agent-exec (sandbox, sandboxPolicy, permissions, allowlist strings); .cursor/sandbox.json and .cursor/sandbox-policies/; sandbox_policy protobuf field; sand_agent_network, sand_network_controls, sand_box_egress_tunnel, sand_action_audit_logs flags; SandBoxService (aiserver.v1). SRC. High.
- Permission modes: "Ask Every Time", "Allowlist", "Allowlist (with Sandbox)", "Allowlist details"; allowlist_in_ask_every_time_mode and smart_allowlist_required flags. SRC. High.
- Filesystem and watching: cursor-file-service (indexing and retrieval); watcherMain.js; @parcel/watcher. SRC. High.
- Worktrees: .cursor/worktrees and .cursor/worktrees.json; cursor.worktreeMaxCount, cursor.worktreeCleanupIntervalHours, cursor.worktreesGlobalMaxSizeGb settings; composer.openTerminalInWorktree; cursor-worktree-textmate. SRC. High.
- Git operations: aiserver.v1 GitIndexService and GitGraphService; tools set_active_branch, pr_management, edit_pr_labels, blame_by_file_path; cursor-checkout (branch migration); cursor-commits; cursor.blame and cursor.fileblame commands; git_snapshot_indexing flag. SRC. High.
- Cloud environments and secrets: .cursor/environment.json; tool replace_env; cloud_agent_keyring_ui and cloud_agent_docker_build_secrets_enabled flags. SRC. High.
- Remote development: anysphere.remote-ssh, remote-wsl, remote-containers, remote-tunnels mappings (extensionReplacementMapForImports); cursor-socket (TCP and TLS provider); tunnels (cursor-tunnel.exe, code-tunnel.exe). SRC. High.
- Deploy and preview: cloud_agent_preview_port_forward flag; Vercel connect pill flags. SRC. Medium.
- Language intelligence: VS Code language features (typescript, json, css, html, markdown, python); anysphere.cursorpyright replaces ms-python.vscode-pylance; anysphere.cpptools replaces ms-vscode.cpptools; LinterService (aiserver.v1). SRC. High.
- Mobile agent surfaces: sand_mobile_agent_computer_console, sand_mobile_haptics_settings, sand_mobile_unread_on_avatar, sand_mobile_feedback_kill_switch flags. SRC flags. Low.

### 7. Coding capability
- Edit tool: edit_file_v2 (ToolCall edit; rawArgs with target_file and code_edit); tool_execution_edit. SRC. High.
- Apply and diff: apply_agent_diff (applyAgentDiffToolCall); aiserver.v1 FastApplyService; "Apply diff" label. SRC. High.
- Tab completion: aiserver.v1 CppService (cpp_suggestion, CppAcceptEventNew, CppSuggestEvent); cursor.cpp.disabledLanguages setting; cursor.suggestcpp and cursor.rejectcppsuggestion commands; cpp_perf_instrumentation and cpp_telem_chunking flags. SRC. High.
- Next-edit prediction: aiserver.v1 CursorPredictionService. Purpose INF. SRC presence. Medium.
- Shadow workspace lint loop: cursor-shadow-workspace extension (shadowServer, shadowHealthCheck, lints_result, linter_errors); aiserver.v1 ShadowWorkspaceService.GetLintsForChange; tool read_lints; glass_editor_shadow_isolation flag. SRC. High.
- Diff review UI: cursor.reviewchanges and cursor.tinderdiffeditor commands; diff_tab_viewport_virtualization flag; glass_diff_ci_tab, glass_diff_commits_tab, glass_diff_reviews_tab flags; hide_inline_changed_files flag. SRC. High.
- Codebase indexing and retrieval: cursor-retrieval (ripgrep grep client, chunking, Merkle-tree sync, embeddings, reranker, tree-sitter chunking; command cursor.grepClient.debug); cursor-file-service; aiserver.v1 CodebaseSnapshotService and FileSyncService; instant_grep_indexing and instant_grep_user_search flags; disable_codebase_indexing flag. SRC. High.
- Search and read tools: semantic_search_full (semSearchToolCall); glob_file_search (globToolCall); ripgrep_raw_search (grep); read_file_v2; list_dir_v2; delete_file. SRC. High.
- Test and run loops: no dedicated test tool found; execution runs through run_terminal_command_v2. INF from absence. Medium.
- PR and review bots: aiserver.v1 BugbotService and BugbotAdminService; bugbot_* flags (autorun kill switch, fail check on findings, merged PR scan, Azure DevOps settings, learning defaults); cursor.runEditorBugbot; review.cursor.com. SRC. High.
- Report bugs and bugfix results: report_bug, report_bugfix_results, record_ci_investigation_findings (ToolCall map). SRC. Medium.
- AI attribution: ai_attribution tool; ai_code_tracking_* flags (v2 scoring, commit timestamps, git operations binding); cursor-commits extension. SRC. High.
- Hallucinated-function checks: aiserver.v1 HallucinatedFunctionsService (method v0ChainRun). Purpose INF. SRC presence. Low.
- Linting and LSP: LinterService (aiserver.v1); cursor-shadow-workspace lints. SRC. Medium.

### 8. Memory, learning and personalization
- Memories: aiserver.v1 ISuggestMemories (suggestion service) and listAutomationMemories; MEMORIES enum with label "Memories". SRC. Medium.
- Memory dreaming: sand_memory_dreaming flag only (consolidation purpose INF). SRC flag. Low.
- Personal environment file: cursor.personalenvironmentjson command. SRC. Medium.
- User instructions and prompt context: generate_user_instructions, drop_custom_prompt_context, agent_context_injection, enable_project_layouts_in_system_prompt flags. SRC. Medium.
- Learned rules for Bugbot: bugbot_onboarding_high_effort_learning_defaults_team_v2 flag. SRC. Medium.
- Conversation search tool and transcript search: search_conversations; agent_conversation_search_tool and local_agent_transcript_search_in_glass flags. SRC. High.
- Reflection tool: reflect. SRC. Medium.
- Learning from user demonstrations: sand_teach_by_demonstration flag. SRC flag. Low.

### 9. Models and routing
- Model slugs present in client bundles (SRC presence; these may be legacy defaults, not current choices):
  - Claude: claude-opus-5, claude-opus-4-8, claude-opus-4-7, claude-opus-4-6-v1, claude-opus-4-5-20251101-v1, claude-opus-4-1-20250805-v1, claude-opus-4-20250514-v1, claude-sonnet-5, claude-sonnet-4-6, claude-sonnet-4-5-20250929-v1, claude-sonnet-4-20250514-v1, claude-haiku-4-5-20251001-v1, claude-fable-5, claude-3.5-sonnet.
  - OpenAI: gpt-5.5, gpt-5.3-codex-high, gpt-4o, gpt-4o-mini, gpt-3.5-turbo, o3-mini.
  - xAI: grok-4.5, grok-4.6, grok-4.6-medium, grok-composer-2, grok-composer-2.5.
  - Google: gemini-2.5-pro, gemini-2.5-flash, gemini-1.5-flash, gemini-1.5-flash-8b.
  Pointers: out/vs/workbench/*.js, extensionHostProcess.js. Tag SRC. High for presence, low for current defaults.
- Auto and max modes: composer_auto_routing_result_display, disable_smart_auto_show_routed_model, auto_disable_unused_models flags; "Max Mode" label; "Use multiple models" label. SRC. High.
- Model picker: composer.selectedModel setting; cli_model_picker, cloud_agent_default_model_picker, web_cloud_agent_new_model_picker, model_picker flags; openGlassAndSwitchModelSlug command. SRC. High.
- Admin model controls: blockedModels, byokDisabled (admin settings in workbench bundle). SRC. High.
- Prompt caching and reasoning hints: cache_control, promptCache, effort_level literals. SRC. Medium.
- Structured tool schemas: zod, zod-to-json-schema; mcp_input_schema_json flag. SRC. Medium.
- Inference services (aiserver.v1): InferenceService, AiService, AiProjectService, AiBranchService, FastApplyService, CppService, CmdKService. SRC. High.
- Private inference: cursor_private_inference_download_prompt and cursor_cli_private_inference_download_prompt flags; cursor-local-agent-runtime description. Meaning (a local or private model runtime with a download prompt) is INF. SRC names. Low.
- Usage and cost: usage limit status and active grants (fetch-usage code path); TeamCreditsService, UsageSimulationService; enable_spend_alerts_percentage_threshold flag; limit_hit_ui_kill_switch flag; sand_usage_page flag; self_serve_team_tiered_pricing_* flags. SRC. High.

### 10. Transparency, observability and control
- RPC and debug logs: cursor.rpcFileLogger.enabled and .folder settings; cursor-rpc-devtools; TraceService; traceSpanService; local_request_tracing and local_request_trace_dd_slogs flags (dd_slogs names a Datadog path); issue_traces_enabled; large_proto_logging_enabled; exthost_rpc_metrics. SRC. High.
- Monitors: cursor.memorymonitor.jsHeapUsedMB, cursor.cpumonitor.totalCpuPct, cursor.debuggingData.contents and jsHeapUsedMB, cursor.debug.timeoutPrevention, cursor.os.* (sleep and suspend metrics); disk_usage_monitor and cpu_monitor_process_snapshot flags. SRC. High.
- NDJSON debug-log ingest: cursor-ndjson-ingest (5 commands; settings ndjson.port with range 7242-7942, ndjson.bindAddress with a warning about non-localhost binding; writes workspace/.cursor/debug.log). SRC. High.
- Hook execution log: customize_hooks_execution_log_detail flag. SRC. Medium.
- Context visualizer: context_visualizer and context_usage_canvas flags. SRC. Medium.
- Evals and replay services: EvalTrackingService, RequestReplayService, ReplayChatService. SRC presence. Medium.
- Telemetry: Statsig (console.statsig.com, statsigapi.net, api.statsigcdn.com; product.json statsigLogEventProxyUrl https://api3.cursor.sh/tev1/v1); Sentry (@sentry/electron; ingest host org-scoped, value not recorded); OpenTelemetry OTLP exporter; enabledTelemetryLevels {error: true, usage: true}; privacy mode suppresses Sentry reports ("skipping report because privacy mode is enabled"). SRC. High.
- Online metrics: OnlineMetricsService; cursor-commits. SRC. High.

### 11. Safety, permissions and governance
- Run mode is admin-controllable: label "Run Mode Disabled by Team Admin"; isDisabledByAdmin and isAdminControlled settings. SRC. High.
- Enterprise admin: aiserver.v1 EnterpriseAdminService; admin_command_denylist (plus enforcement kill switch); admin_network_controls; mcp admin flags; browser_features_access_control; codebase_protection_* (client enforcement and reporting configs by plan: pro, pro_plus, ultra); extension_signature_verification; managed_* flags; team permission strings (team.background_composers.manage, team.full_self_driving.manage, team.public_profile_settings). SRC. High.
- Admin browser and workspace protection: dotCursorProtection (admin setting; behavior unverified); browserOriginAllowlist. SRC. High for names.
- Secrets handling: keytar (OS credential store); CLI option use-inmemory-secretstorage; replace_env tool; cloud_agent_keyring_ui flag. SRC. High.
- Privacy and ghost mode: x-ghost-mode header; ghost_mode setting (17 hits); "Privacy Mode" label; privacy-mode state gates Sentry. SRC. High.
- Zero data retention: no ZDR literal found. INF from absence. Low.
- Prompt-injection defenses: no explicit defense found by name. Hook points beforeReadFile, beforeMCPExecution and beforeShellExecution exist. INF. Low.
- Link and extension trust: linkProtectionTrustedDomains; trustedExtensionAuthAccess; trustedExtensionPublishers (anysphere, microsoft, github, and others); cursorTrustedExtensionAuthAccess. SRC. High.
- Extension signing: extension_signature_verification flag; SIGNING_REGISTRY_MAP in out/main.js maps marketplace.cursorapi.com to open-vsx.org. SRC. High.
- Web Bot Auth signing: sand_web_bot_auth_signing flag only. SRC flag. Low.
- Permission modes: section 6 (Ask Every Time, Allowlist, Allowlist with Sandbox). SRC. High.

### 12. UI/UX and product design
- Layout and mode variants: Zen Mode (hides editor tabs); Editor Mode; Design Mode ("Open in Design Mode" for the Glass window); Compatibility Mode (HTTP compatibility option); agentIdeUnification settings (unifiedSidebarVisible, sidebarLocation); toggleAgentWindowIDEUnification command; windowSwitcher settings. SRC. High.
- Glass window: separate renderer bundle plus 111 glass_* flags (sidebar, projects, diffs, cloud, PDF, video, theme). SRC. High.
- Themes: 12 theme extensions; cursor_oklch_color_tokens flag; glass_custom_theme_support flag. SRC. High.
- Onboarding and dashboards: OnboardingChecklistService; DashboardService; dashboard_individual_onboarding_checklist and dashboard_agent_requests_heatmap flags. SRC. High.
- Performance patterns: diff_tab_viewport_virtualization; @tanstack/react-virtual and virtual-core; glass_lazy_file_tabs; glass_react_commit_churn. SRC. High.
- Internationalization: out/nls.messages.json (VS Code translations); locales/*.pak (about 50 Chromium locales). Cursor-specific workbench strings are English. SRC. Medium.
- Accessibility: out/vs/platform/accessibilitySignal; terminal accessible-view settings. SRC. Medium.
- Notifications: composer.notifications.pendingAction; sand_notify_bus and sand_notify_safety_poll flags; green_dot_automation_status flag. SRC. Medium.
- Keybindings and input: keybinding_migration_killswitch flag; ide_cmd_enter_submit flag; chat.submit setting. SRC. Medium.

### 13. Platform architecture
- Process model: Electron main (out/main.js); renderers (workbench bundles); utility processes (conversationSearch, mcpProcess, sharedProcess, extensionMonitor, processExplorer, watcher); extension host (extensionHostProcess.js). SRC and REA. High.
- Agent services over protobuf (agent.v1, nine services): AgentService, AgentHostService, ControlService, LifecycleService, ExecService, PtyHostService, TmuxSessionService, PrivateWorkerApiService, PrivateWorkerBridgeExternalService. SRC. High.
- Backend services over protobuf (aiserver.v1, 65 services): BackgroundComposerService, AutomationsService, SchedulerService, ChatService, AiService, CppService, CmdKService, FastApplyService, ShadowWorkspaceService, BugbotService, MCPRegistryService, MarketplaceService, EnterpriseAdminService, GitIndexService, GitGraphService, CodebaseSnapshotService, FileSyncService, SandBoxService, VmDaemonService, and others. Full list via strings on out/main.js. SRC. High.
- Transport: Connect-RPC over HTTP/2 (http2 flags: disableHttp2, http1_keepalive_disabled); SSE fallback (disableHttp1SSE); x-cursor-* headers. SRC. High.
- Storage: SQLite state.vscdb (@vscode/sqlite3); the composer conversation table has workspaceId, composerId, isSubagent, isArchived, recency, checkpointAt, with indexes; IndexedDB (3 references). SRC (schema in code; the user database was NOT read). High.
- Project-local state: .cursor/ (rules, skills, cloud-skills, skills-cursor, agents, commands, plans, worktrees, worktrees.json, sandbox.json, sandbox-policies, environment.json, mcp.json, hooks.json, projects, plugins/cache, debug.log). SRC. High.
- Update and distribution: api2.cursor.sh/updates; cursor.update.* commands (win32 promotion, restart markers); cursor_update_supervisor flag; tools/inno_updater.exe; Inno Setup uninstaller. SRC. High.
- Remote authority: cursor-resolver (background-composer), cursor-resolver-helper (connection tokens), cursor-socket. SRC. Medium.
- Rust-portable design notes (INF): main, renderer and utility process split; protobuf-over-HTTP/2 RPC; local SQLite state; MCP servers as child processes; PTY and tmux session hosts; sandbox policies with allowlists; hooks as lifecycle callbacks; per-agent worktrees.

Total feature candidate bullets in section 3: see the count recorded in the final JSON (itemsListed).

---

## 4. Extension points

- MCP servers: mcp.json (user and project), .cursor/mcp.json, deep-link install (mcp.deeplinkInstall), transports stdio, SSE and streamable HTTP, OAuth, admin allowlists. SRC.
- Hooks (hooks.json; events in out/vs/workbench/api/node/extensionHostProcess.js, object Ze): beforeShellExecution, afterShellExecution, beforeMCPExecution, afterMCPExecution, beforeReadFile, afterFileEdit, beforeTabFileRead, afterTabFileEdit, beforeSubmitPrompt, afterAgentResponse, afterAgentThought, stop, sessionStart, sessionEnd, preCompact, subagentStart, subagentStop, preToolUse, postToolUse, postToolUseFailure, workspaceOpen. SRC. High.
- Rules: .cursor/rules/*.mdc (frontmatter), legacy .cursorrules, AGENTS.md, CLAUDE.md. SRC. High.
- Skills: .cursor/skills/, .cursor/cloud-skills/, .cursor/skills-cursor/; skill publish and unpublish as plugin packs. SRC. High.
- Plugins: .cursor/plugins/ and cache; marketplace; third-party and Claude Code plugin import. SRC. High.
- Custom subagents: .cursor/agents/. Custom modes: cloud_custom_modes and glass_custom_modes flags. SRC. Medium.
- Commands: .cursor/commands/. SRC. Medium.
- Environment and worktree config: .cursor/environment.json, .cursor/worktrees.json, .cursor/sandbox.json, .cursor/sandbox-policies/. SRC. High.
- Ignore files: .cursorignore (43 hits) and .cursorindexingignore (3 hits). SRC. High.
- VS Code extension API: out/vscode-dts/vscode.d.ts and 54 enabled API proposals. Extension gallery galleryId "cursor" at marketplace.cursorapi.com; signing via Open VSX. SRC. High.
- Deep links: cursor:// with handlers cursor-deeplink (automations route) and cursor-mcp (MCP install). SRC. High.
- Automations webhooks: webhook payload forwarded to the run context (workbench.anysphere-ui-automations.js). SRC. High.
- CLI: resources/app/bin/cursor; cli.js options; cursor.installCli command; desktop bridge. SRC. Medium.
- Local HTTP: NDJSON ingest server (ports 7242-7942, default localhost); tunnels (cursor-tunnel.exe). SRC. High.
- Public SDK or API: none found in the install. INF. Low.

Cursor extension list (20 cursor-* extensions; description from package.json):
1. cursor-agent-exec (anysphere): agent execution (commands, files, tools) with user permissions and approvals.
2. cursor-agent-host (anysphere): hosts agent orchestration in the AgentExec extension host.
3. cursor-agent-worker (anysphere): installs and runs the agent worker at startup.
4. cursor-always-local (anysphere): experimentation features; activates on background-composer remote authority.
5. cursor-browser-automation (cursor): MCP server for browser automation (19 tools).
6. cursor-checkout (anysphere): checkout provider for branch migration.
7. cursor-commits (anysphere): tracks requests and commits for online metrics.
8. cursor-deeplink (anysphere): handles deep-link URIs.
9. cursor-explorer (anysphere): workspace extension for Cursor Explorer.
10. cursor-file-service (anysphere): indexing and retrieval (no activation events).
11. cursor-local-agent-runtime (anysphere): hosts "Cursor Private Inference" outside workspace extension hosts.
12. cursor-mcp (anysphere): handles MCP.
13. cursor-ndjson-ingest (anysphere): NDJSON log ingest server (5 commands, 2 settings).
14. cursor-polyfills-remote (anysphere): polyfills for the workspace extension host.
15. cursor-resolver (anysphere): background-composer remote authority resolver.
16. cursor-resolver-helper (anysphere): connection token provider for the resolver.
17. cursor-retrieval (publisher not declared): indexing and retrieval (commands cursor.grepClient.debug, cursor.codebaseTelemetry.triggerSnapshot; setting cursor-retrieval.canAttemptGithubLogin).
18. cursor-shadow-workspace (anysphere): shadow workspace for lint feedback (manifest has unresolved %displayName% placeholders).
19. cursor-socket (anysphere): TCP and TLS socket provider.
20. cursor-worktree-textmate (everysphere publisher): TextMate highlighting for .cursor/worktrees files.
Source for all: resources/app/extensions/<name>/package.json (SRC). Parsed manifests: scratch tmp/cursor/manifests.json.

---

## 5. Tool catalog (client ToolCall type to tool name; SRC from out/vs/workbench/*.js and out/main.js)

Mapped names (some are protocol or UI state names, not model tools; treated as candidates): adopt, ai_attribution, apply_agent_diff, ask_question, blame_by_file_path, computer_use, connect_scm, create_agent, create_goal, create_plan, delete_file, edit_file_v2, edit_pr_labels, fetch, fetch_cloud_agent_data, fetch_mcp_resource, generate_image, get_agent_status, get_mcp_tools, glob_file_search, list_dir_v2, list_mcp_resources, mcp_auth, pi_bash_tool_call, pi_edit_tool_call, pi_find_tool_call, pi_grep_tool_call, pi_ls_tool_call, pi_read_tool_call, pi_write_tool_call, pr_management, read_agent_transcript, read_file_v2, read_lints, record_ci_investigation_findings, record_screen, reflect, replace_env, report_bug, report_bugfix_results, ripgrep_raw_search, run_terminal_command_v2, search_conversations, semantic_search_full, send_final_summary, send_to_agent, send_to_user, set_active_branch, setup_vm_environment, start_grind_execution, start_grind_planning, stop_agent, switch_mode, task_v2, todo_read, todo_write, tool_execution_edit, tool_execution_shell, tool_execution_task, update_current_step, update_goal, web_fetch, web_search, write_shell_stdin.
- Other tool names seen in the bundle: run_terminal_cmd and edit_file (older definitions with target_file and code_edit arguments), read_file (in a shell-like tool list).
- pi_* set (pi_bash, pi_edit, pi_find, pi_grep, pi_ls, pi_read, pi_write): mirrors a bash, read, write, edit, find, grep, ls set; purpose and which models use it INF. Low.
- Noise removed: await, text, truncated (regex artifacts).

---

## 6. Data and protocol hints

- Wire: Connect-RPC with protobuf over HTTP/2; streams use SSE fallback; x-cursor-* request headers: client-version, client-type, client-os, client-os-version, checksum, team-id, timezone, server-region, config-version, draggable. SRC.
- Endpoints (product.json and bundle; SRC):
  - Cursor sites: cursor.com, www.cursor.com, review.cursor.com, changelog.cursor.com, downloads.cursor.com, origin.cursor.com.
  - Cursor services: api2.cursor.sh (updates, extensions-control), api3.cursor.sh/tev1/v1 (Statsig event proxy), api4.cursor.sh, agent.api5.cursor.sh with regional variants agentn-gcpp-uswest, agentn-gcpp-eucentral, agentn-gcpp-apsoutheast, repo42.cursor.sh, prod.authentication.cursor.sh.
  - Staging: staging.cursor.sh, dev-staging.cursor.sh, staging.cursor.com, review.stg.cursor.com, staging-marketplace.cursor.sh.
  - Marketplace and downloads: marketplace.cursorapi.com, marketplace.cursor.sh, cursor-cdn.com, cursor.blob.core.windows.net (remote server releases), cursorapi.com (backup updates).
  - Third parties: open-vsx.org (signing registry), api.github.com (graphql, search), github.com app install, console.statsig.com, statsigapi.net, api.statsigcdn.com, sentry.io ingest (org-scoped; value not recorded), us5.datadoghq.com (string in profiling link), slack.com app redirect.
- Auth: CursorAuthenticationService and CursorAuthToken names (72 hits; token values not recorded); MCP OAuth (oauth-attempt state in globalState and secrets); keytar (OS credential store); MSAL (@azure/msal-*); GitHub and Microsoft auth via VS Code auth providers (trustedExtensionAuthAccess lists). SRC.
- Privacy and ghost mode: x-ghost-mode header; ghost_mode setting; privacy-mode state gating Sentry reports. SRC.
- Telemetry: Statsig gates (641 client flags); Sentry (electron SDK and replay worker); OTLP trace exporter; analytics, client-logger, metrics and performance-event services; enabledTelemetryLevels {error, usage}. SRC.
- Storage: state.vscdb (SQLite) with a composer conversation table (fields include workspaceId, composerId, isSubagent, isArchived, recency, checkpointAt); IndexedDB; .cursor/ project folders; NDJSON log at workspace/.cursor/debug.log. The user database and any stored values were NOT read. SRC (schema in code).
- Feature flags: 641 client-side names in bundles; saved list at scratch tmp/cursor/flags_client.txt. Prefix counts: glass 111, sand 44, cloud 42, enable 28, agent 19, mcp 18, composer 15, cursor 14, bugbot 12, cli 10, web 9, disable 9, ai 9, local 8, codebase 8, automations 8. SRC.
- Protobuf services: aiserver.v1 (65 services) and agent.v1 (9 services) in out/main.js and extensionHostProcess.js. SRC.
- Hook payloads, MCP wire formats: JSON and JSON-RPC via @modelcontextprotocol/sdk; the NDJSON ingest server is HTTP on localhost. SRC/INF.

---

## 7. Blockers and limits

- Server side not visible: agent prompts, tool routing, model choice logic and model weights live on the backend (aiserver.v1, agent.api5). The install shows only client descriptors and names. INF from the install shape.
- Minified bundles: most identifiers are mangled, so many behaviors are INF from names, flags and labels.
- IPC channels mostly dynamic: REA found 5 literal and 12 dynamic channel operations. The empty-seed channel trace did not match. Grep found only VS Code channels (vscode:openDevTools, vscode:extensionProcessSpawned, vscode:registerAuxiliaryWindow).
- REA coverage partial: semantic layer truncated; 2 parse failures; 2,514 semantic unknowns; module view lists 48 nodes, not the full graph; PE payloads opaque (only structure); open_binary avoided to keep the shared active target clean.
- Standalone agent CLI binary absent: resources/app/bin/cursor is a launcher and out/cli.js is a VS Code-style CLI with agent, cloud and sandbox option names. The agent CLI feature set comes from cli_* flags and is INF.
- Flags: 641 client-side flags show defaults in the bundle; server overrides are unknown.
- User data intentionally not read: AppData/Roaming/Cursor (state.vscdb, Local Storage, IndexedDB, credentials) was skipped per rules. Anything visible only through user data is not covered.
- Unverified purposes (low confidence): Private Inference (cursor-local-agent-runtime, cursor_private_inference flags); Full Self-Driving; Canvas; HallucinatedFunctionsService; CursorPredictionService; memory dreaming; codebase_protection enforcement; dotCursorProtection behavior; pi_* tool set; react-vnc use.
- Grok Bot: the brief says Grok Bot shares the @anysphere codebase. This scout found GrokBotService and grok_bot_* flags inside Cursor, but did not inspect the separate Grok Bot app. The relation is unconfirmed here.
- node_modules size was not measured (the background du did not report it). No docs fetched (DOC: none).
- Unverified claims left out: "Max Mode" and "Multitask" behaviors beyond labels; Deep research (absent); ZDR (absent).

---

## 8. Notable leads for the deep dive (pointers only)

1. Agent loop and tool contract: section 5 tool names; out/vs/workbench/workbench.desktop.main.js (ToolCall mapping; search for "askQuestionToolCall" and "editFile"); edit_file_v2 target_file and code_edit arguments.
2. Apply, edit and lint loop: aiserver.v1 FastApplyService, CmdKService, CppService, ShadowWorkspaceService; cursor-shadow-workspace extension; apply_agent_diff.
3. Retrieval: cursor-retrieval/dist (merkle, chunk, embedding, reranker, treesitter, ripgrep client); cursor-file-service; CodebaseSnapshotService and FileSyncService.
4. Background and cloud agents: BackgroundComposerService, VmDaemonService, SandBoxService, cloud_environment_template, cursor-resolver; agent.v1 service set in extensionHostProcess.js.
5. Hooks and permissions: hook event list (extensionHostProcess.js, object Ze); cursor-agent-exec/dist (permissions, allowlist, sandboxPolicy, hookContextCollector); Ask Every Time, Allowlist, Allowlist (with Sandbox) labels.
6. MCP and extensibility: cursor-mcp/dist (OAuth, stdio, streamableHttp, sse); out/vs/code/electron-utility/mcpProcess/mcpProcessMain.js; mcp.json, hooks.json, .cursor/rules, skills and plugins layout.
7. Browser and computer use: out/vs/platform/browserView/electron-main/preload-browser.js; cursor-browser-automation/dist (19 browser_* tools); browser_navigate allowlist check; computer_use oneof; sand_computer_use_playwright flag.
8. Models and governance: model slug literals; admin settings (Run Mode, blockedModels, byokDisabled, mcp allowlists, codebase_protection_* plans); usage and grants.
9. Protocol: aiserver.v1 and agent.v1 service descriptors in out/main.js and extensionHostProcess.js; x-cursor headers; Connect over HTTP/2 with SSE fallback.
10. Platform: Electron 40.10.3 / Chromium 144; utility process split; SQLite state schema (checkpointAt, isSubagent); Glass window bundle (workbench.glass.main.js) as the agent-first surface.
