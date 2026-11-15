# Google Antigravity (agent hub desktop app, build 2.11.0): inventory scout

Scope: installed program at C:/Users/ImaFernandesDaCosta/AppData/Local/Programs/antigravity (read-only). Pre-extracted asar copy: scratchpad/extract/antigravity/ (dist/, node_modules/). Nothing was launched, no project code was run, and no user-data stores were read (~/.gemini/* and AppData/Roaming/Antigravity were skipped; the theme and config paths were noted from code only).

## Legend

- Provenance tags: REA = REA tool output on a shipped artifact. SRC = read directly in JavaScript source or package files. SRC-STR = identifier or string present in a binary, found with read-only grep -a (presence only; not REA; not proof of use; strings are sometimes concatenated, so names can carry noise). DOC = public documentation. INF = inference (basis given).
- Confidence = how sure the capability exists in this build, not how well it works. high = identifier plus a clear code path or copy; med = identifier plus context; low = name only.
- Pointer prefixes: [SHELL] = resources/app.asar -> dist/*.js. [LS] = resources/bin/language_server.exe. [CDM] = resources/app.asar.unpacked/node_modules/chrome-devtools-mcp/build/src. [WEB] = resources/bin/webm_encoder.exe. [DOC-GS] = https://antigravity.google/docs/getting-started. [DOC-EL] = https://www.electronjs.org/docs/latest/tutorial/fuses.
- Every feature candidate in section 3 is one bullet starting with "- [" so it can be counted.

## 0. Key correction and naming

- This install is the agent-first desktop app, not the VS Code-based editor. [SHELL dist/ideInstall/wizardHtml.js] The first-run wizard says the app was redesigned to put agents first, and that a code editor is available as a separate app named Antigravity IDE. Programs/ has no "Antigravity IDE" folder, and no Antigravity install was found under Program Files (two paths checked, listing only). So the editor is NOT on this PC and was NOT inventoried. Its editor-side bridge is visible here as exa.extension_server_pb (section 3.5).
- Naming: docs and LS strings say "Antigravity 2.0" [LS, SRC-STR: "agy CLI, the Antigravity IDE, or Antigravity 2.0"]. The build is 2.11.0 [SHELL package.json; REA-PE RT_VERSION ProductVersion 2.11.0.0]. INF: 2.0 is the product generation, 2.11.0 is the build number.

## 1. Identity and stack

- Product: Antigravity (productName), package "antigravity", version 2.11.0, publisher Google, copyright 2026 Google. [SHELL package.json; REA-PE RT_VERSION]
- Shell: Electron 41.0.2 with Chromium 146.0.7680.72. [SRC-STR version markers in Antigravity.exe; DOC-EL for fuse semantics]. Squirrel installer marker SquirrelAwareVersion=1 [REA-PE].
- Backend: language_server.exe (153,057,280 bytes, PE32+, machine 0x8664). Go version strings go1.26 and go1.28 appear [SRC-STR]. It serves the web UI over HTTPS on 127.0.0.1 at a dynamic port [SHELL languageServer.js, constants.js WINDOW_ORIGIN].
- Recorder: webm_encoder.exe (13,615,104 bytes). Subscribes to DevTools screencast frames and writes WebM (VP9) [WEB strings: screencastFrame, webm, vp9, DevToolsActivePort, --remote-debugging-port, --headless]. No PE resources [REA-PE].
- Browser MCP server: chrome-devtools-mcp 1.1.0 (Apache-2.0, Google LLC), shipped unpacked [CDM package.json; REA-JS module list].
- Shell source: TypeScript compiled to CommonJS (dist/*.js with __esModule helpers) [SHELL].
- Stack by layer:
  - Shell: Electron 41, TypeScript, electron-log, electron-updater, Connect RPC (@connectrpc/connect and connect-node), protobuf-es (@bufbuild/protobuf), shell-env [SHELL package.json].
  - Backend: Go, Google "gemini_coder" monorepo module tree, protobuf-go, Connect/gRPC strings [LS, SRC-STR].
  - UI: web app built from a TypeScript "agent_ui_toolkit" source tree. Framework not verified (React string hits are noisy) [LS, SRC-STR, low].
  - Browser automation: chrome-devtools-mcp. Dev-only dependency list: puppeteer 25.1.0, lighthouse 13.3.0, chrome-devtools-frontend 1.0.1632065, @modelcontextprotocol/sdk 1.29.0, @google/genai ^2.0.1, yargs 18.0.0 [CDM package.json]. Runtime is the bundled build/src [INF].

### 1a. Dependency list grouped by capability

- Declared runtime dependencies (7): @bufbuild/protobuf ^2.13.0, @connectrpc/connect ^2.1.2, @connectrpc/connect-node ^2.1.2, chrome-devtools-mcp ^1.1.0, electron-log ^5.4.3, electron-updater ^6.8.3, shell-env ^4.0.3 [SHELL package.json].
- RPC and protobuf: @connectrpc/* (protocol-grpc, protocol-grpc-web, protocol-handler-factory seen in the REA module list), @bufbuild/protobuf (codegenv1, wire, wkt) [REA-JS; SHELL node_modules].
- Updates and distribution: electron-updater (includes PrivateGitHubProvider and RpmUpdater modules), builder-util-runtime, js-yaml (update config), semver [SHELL node_modules; REA-JS].
- Logging and diagnostics: electron-log, debug, ms, ansi-regex, strip-ansi, strip-final-newline [SHELL node_modules].
- Process and environment: shell-env, default-shell, execa, cross-spawn, which, isexe, shebang-command, shebang-regex, npm-run-path, path-key, signal-exit, human-signals, onetime, mimic-fn, get-stream, merge-stream, is-stream, lazy-val, tiny-typed-emitter [SHELL node_modules].
- Files and data: fs-extra, graceful-fs, jsonfile, universalify, sax, lodash.escaperegexp, lodash.isequal, argparse [SHELL node_modules].
- Browser bundle internals (REA module list): ToolHandler, McpContext, McpPage, HeapSnapshotManager, PageCollector, TextSnapshot, WaitForHelper, telemetry/ClearcutLogger, telemetry/watchdog, daemon/, slim/ [CDM build/src; REA-JS].

## 2. Directory and module map

### 2a. Install root (Programs/antigravity)

- Antigravity.exe (222,848,000 B): Electron host with ELECTRONASAR integrity resource, version info, icon group, manifest [REA-PE].
- resources/app.asar (4,526,306 B): main-process bundle, see 2b [SHELL].
- resources/app.asar.unpacked/node_modules/chrome-devtools-mcp (about 17 MB): unpacked MCP server [CDM].
- resources/bin/language_server.exe (153 MB): agent backend, RPC server, UI host [LS].
- resources/bin/webm_encoder.exe (13.6 MB): browser recording encoder [WEB].
- resources/app-update.yml (updater feed), elevate.exe (107,520 B, installer elevation helper), locales/ (55 Chromium .pak locales, af to zh-TW), resources.pak, chrome_100/200_percent.pak, icudtl.dat, ffmpeg.dll, libEGL.dll, libGLESv2.dll, vk_swiftshader.dll, vulkan-1.dll, d3dcompiler_47.dll, dxcompiler.dll, dxil.dll, snapshot_blob.bin, v8_context_snapshot.bin, LICENSES.chromium.html, Uninstall Antigravity.exe [directory listing].

### 2b. app.asar -> dist/ (main process), file -> purpose

- main.js (405 lines): single-instance lock; antigravity:// deep-link protocol; starts the host bridge and the LS; creates window, tray and menu; quit confirmation dialog; window-all-closed honours runInBackground; headless mode [SHELL].
- languageServer.js: LS launcher. Flags, env, port detection (stdout pattern "listening on ... port at N for HTTP or HTTPS"), crash monitor (max 3 restarts in 60 s, 2 s cooldown), OAuth URL capture from LS stdout, --stamp build CL, certificate trust for 127.0.0.1 and localhost only [SHELL].
- hostBridgeServer.js + proto/host_bridge_pb.js: loopback Connect server for LS -> shell callbacks (GetUpdateStatus, ApplyUpdate). Loopback peer check, Host-header check, 32-byte bearer token compared in constant time [SHELL].
- ipcHandlers.js: 28 main IPC handlers (dialogs, updater, notifications, storage, logs, window controls, zoom, shell open, IDE-installed check, agent count) [SHELL; REA-JS: 28 main handlers].
- preload.js: 11 contextBridge APIs: electronUpdater, dialog, nativeNotifications, nativeStorage, logs, extensions, deepLink, agent, electronNative, ide [SHELL; REA-JS context_bridge_apis 11].
- customScheme.js: privileged plugin:// scheme that proxies sidecar or extension localhost origins; the authority map is pushed by the renderer [SHELL].
- updater.js: electron-updater. Channel latest-<arch> (latest-x64-win on Windows); first check 10 s after start, then hourly; autoDownload on; autoInstallOnAppQuit when packaged; headless Linux path replaces the binary and relaunches [SHELL].
- menu.js, keybindings.js, tray.js: File > New Window (CmdOrCtrl+Shift+N), Help > Docs (opens antigravity.google/docs), macOS "Check for Updates", Cmd/Ctrl+Q intercept, tray menu with running-agent count and Open/Quit [SHELL].
- utils.js: createWindow (1400x900, min 500x400, hidden title bar with overlay on Windows height 30, contextIsolation on, nodeIntegration off, devtools only when unpackaged, window.open denied and routed to the system browser). Theme read from the settings file (DARK, LIGHT or INHERIT). SleepBlocker (powerSaveBlocker prevent-display-sleep). agy-node wrapper script (ELECTRON_RUN_AS_NODE=1) added to PATH [SHELL].
- storage.js, services/settingsService.js, types.js: app_storage.json key-value store with change broadcast to all windows. Settings: runInBackground (default off on Windows, on elsewhere), keepComputerAwake (off), autoCheckForUpdates (on). Update-state wire strings [SHELL].
- paths.js, constants.js: data dir ~/.gemini/antigravity (dev build: antigravity-dev), LS log file, DevToolsActivePort in userData, WINDOW_ORIGIN https://127.0.0.1, DYNAMIC_PORT 0 [SHELL].
- ideInstall/*: first-run wizard "Welcome to the new Antigravity!" that downloads and silently installs Antigravity IDE (win32 installer flags /VERYSILENT /MERGETASKS=!runcode), copies user data from ~/.gemini/antigravity to ~/.gemini/antigravity-ide plus a backup dir. Download metadata comes from a Cloud Run host (us-central1) [SHELL].
- loadingOverlay.js: "Loading Antigravity" splash in a WebContentsView until did-finish-load [SHELL].
- *.test.js and test/helpers.js: 12 test files, names only (test-name grep returned nothing) [SHELL].

### 2c. Language-server internal map (SRC-STR: Go module paths and proto packages)

- gemini_coder/framework/agent: agent step handlers, PreInvocationHook, PostInvocationHook, StopHook and StopHookDecision, ToolConverter, ModelAPI.ClassifyError [LS].
- gemini_coder/framework/core: GenerateFullResponse (+WithRetry), InvocationContext, PreToolHook and PostToolHook, Toolbox and NewToolbox, RenderSystemPrompt, Run, StopHookResult [LS].
- gemini_coder/cider/proto/trajectory_steps.proto: StartCodeReviewStep and ProposeAICommentsStep. INF: "cider" is the internal coding-agent lineage behind these step types [LS].
- gemini_coder/agent_ui_toolkit/dev/appVariant/externalAppVariant.ts: UI toolkit source path, referenced from a LINT comment [LS].
- Proto packages (exa.*): 2,989 distinct names in total (grep count of string references, not message counts). Approximate string-reference counts: codeium_common_pb about 1,300; language_server_pb about 1,100 (LanguageServerService); cortex_pb about 1,000 (agent trajectories, steps, tools, "Cascade"); api_server_pb about 380 (ApiServerService); seat_management_pb about 340 (SeatManagementService); extension_server_pb about 180 (ExtensionServerService); index_pb about 145 (IndexService, IndexManagementService); opensearch_clients_pb about 125 (KnowledgeBaseService, CodeIndexService); jetski_cortex_pb about 95 (UI state streams, cost); chat_pb about 70; analytics_pb about 40 (AnalyticsService); vcs_pb, project_pb about 35 each; context_module_pb, reactive_component_pb about 30 each; remoting about 30 (RemotingService); diff_action_pb, code_edit about 30 each; cascade_plugins_pb about 25 (CascadePluginsService); hooks_pb about 20; chat_client_server_pb about 15; config_pb about 12; unified_state_sync_pb about 11; host_bridge_pb, browser_pb about 9 each; prompt_pb about 7; dev_pb about 5 [LS, SRC-STR].
- Shell-to-LS launch flags [SHELL languageServer.js]: --standalone, --override_ide_name antigravity, --subclient_type hub, --override_ide_version, --override_user_agent_name antigravity, --https_server_port, --csrf_token, --app_data_dir, --api_server_url https://generativelanguage.googleapis.com, --cloud_code_endpoint https://daily-cloudcode-pa.googleapis.com, --enable_sidecars, --host_bridge_url, --host_bridge_token, --headless (headless only). Env: AGY_BROWSER_ACTIVE_PORT_FILE, CHROME_DEVTOOLS_MCP_JS (bundled MCP entry point).
- Shell environment switches [SHELL]: ELECTRON_OZONE_PLATFORM_HINT=headless (headless mode), DEV_URL (load a URL directly), JETSKI_LS_PORT (fixed LS port), CODEIUM_LANGUAGE_SERVER_BIN (LS binary override for dev), APPIMAGE (Linux relaunch path).

## 3. Feature candidates by taxonomy category

Format: - [tag, confidence] capability: detail. [pointer]. Tags are defined in the legend.

### 3.1 Chat experience

- [SRC-STR, high] Agent conversations ("Cascade" trajectories): create, fetch-all, delete, cancel. Trajectory types CASCADE, INTERACTIVE_CASCADE, USER_MAINLINE, USER_GRANULAR. [LS exa.cortex_pb: CascadeConfig, CortexTrajectoryType, GetAllCascadeTrajectories, DeleteCascadeTrajectory]
- [SRC-STR, high] Live UI state streaming: StreamAgentStateUpdates with TrajectoryUpdate, StepsUpdate, PendingAgentMessagesUpdate, QueuedStepsUpdate, BackgroundTasksUpdate, ArtifactSnapshotsUpdate, TrajectoryFileDiffsUpdate; reactive stream StreamReactiveUpdates. [LS exa.jetski_cortex_pb, exa.reactive_component_pb]
- [SRC-STR, high] Message queueing while the agent runs (queued user input steps, DeleteQueuedUserInputStep). [LS jetski_cortex_pb QueuedStepsUpdate]
- [SRC-STR, high] Stop and cancel controls: CancelCascadeInvocation, CancelCascadeSteps, ForceStopCascadeTree; StopReason enum (5 values). [LS]
- [SRC-STR, high] Fork a conversation: ForkConversation RPC, ForkFromIdentifier. Copy "Battle Mode is limited to at most N forks for this workspace type". [LS]
- [SRC-STR, med] Battle Mode: parallel arms (forks) of one prompt; a winner is picked and can optionally be applied back to the conversation. Identifiers BattleModeInfo, BattleEndType, EndBattleMode, EliminateBattleModeArm, DetectBattleModeAutoTrigger; flag enable-battle-mode-custom-agents; copy "no arms provided for Battle Mode", "winner conversation not found", "apply_to_conversation_late". Semantics INF from names. [LS]
- [SRC-STR, low] Side questions kept out of the main thread: CORTEX_TRAJECTORY_TYPE_SIDE_QUESTION (name only). [LS enum]
- [SRC-STR, high] Share a conversation by link: CreateTrajectoryShare, FetchTrajectoryShare, DeleteTrajectoryShare; admin gate IsConversationSharingBlocked. [LS api_server_pb]
- [SRC-STR, high] Export a conversation to Markdown: ConvertTrajectoryToMarkdown RPC. [LS]
- [SRC-STR, med] Search over past conversations: TRAJECTORY_SEARCH step (CortexStepTrajectorySearch). [LS cortex_pb]
- [SRC-STR, med] Context and history management: CONVERSATION_HISTORY step, CompactionInfo, ContextWindowMetadata, ConversationHistoryConfig. [LS cortex_pb]
- [SRC-STR, high] Suggested follow-up replies: SUGGESTED_RESPONSES step. [LS cortex_pb]
- [SRC-STR, high] Ask the user a question with selectable options and wait for the answer: ASK_QUESTION step, AskQuestionOption, AskQuestionEntry, AskQuestionInteraction. [LS cortex_pb]
- [SRC-STR, med] Conversation picker and focus across windows: ShowConversationPicker, OpenConversationWorkspaceQuickPick, SmartFocusConversation. [LS exa.extension_server_pb]
- [SRC-STR, med] Agent Manager: in-app guide text describes it as the hub for working with agents and project contexts (workspaces). RPC IsAgentManagerEnabled. DOC-GS lists "Agent overview" and "Agent side panel". [LS; DOC-GS]
- [SRC-STR, med] Auxiliary pane (HTML) with Subagents, Background Tasks and Artifacts sections. [LS prompt and copy strings]
- [SRC-STR, med] Projects and workspaces: CreateProject, DeleteProject, AddTrackedWorkspace, CreateScratchProjectFolder, CreateMagicProject (name only), AddEnvironmentToProject. DOC-GS: "Agents work within projects". [LS; DOC-GS]
- [SRC-STR, low] Import: string "antigravity import" present; scope unverified. [LS]
- [SRC-STR, med] In-product feedback: RecordChatFeedback, ProvideFeedback, RecordChatPanelSession RPCs. [LS api_server_pb]
- [SHELL, high] Native notifications; a click focuses the window and sends a payload to the renderer. [SHELL ipcHandlers.js notification:send, notification:clicked]
- [SHELL, high] Deep links: antigravity:// URLs focus the window and forward the URL to the renderer (deep-link event). [SHELL main.js, preload.js deepLink API]
- [SHELL, high] Persistent zoom with zoom in, zoom out and reset; level stored under zoomLevel. [SHELL ipcHandlers.js, utils.js]

### 3.2 Multimodal and creation

- [SRC-STR, high] Image generation: GENERATE_IMAGE step; GetImageGeneration RPC; image models gemini-3.1-flash-image (and -preview), gemini-3-pro-image, gemini-2.5-flash-image-preview. [LS]
- [SRC-STR, high] Media artifacts: MediaR, ImageData, DeleteMediaArtifact. [LS]
- [SRC-STR, med] Voice dictation: copy "DICTATION"; GetTranscription RPC; StartAudioRecording, EndAudioRecording, GetCurrentAudioRecording, EndAudioSession. [LS extension_server_pb, api_server_pb]
- [SRC-STR, med] Text-to-speech: copy "Text To Speech"; env name GEMINI_TTS_VOICE_REPLICATION. [LS]
- [SRC-STR, high] File and image upload: WRITE_BLOB step, ImageData, NotebookBlob; upload_file tool. [LS; CDM]
- [SRC-STR, high] Screenshots as visual evidence and for self-verification: CAPTURE_BROWSER_SCREENSHOT step, take_screenshot and screenshot tools. DOC-GS lists "Visual screenshots" under Artifacts. [LS; CDM; DOC-GS]
- [SRC-STR, high] Browser session video recording (WebM/VP9): webm_encoder.exe; LS McpBrowserRecordingStop; copy "failed to generate browser recording video"; DOC-GS lists "Browser recordings". [WEB; LS; DOC-GS]
- [SRC-STR, high] Jupyter notebooks: READ_NOTEBOOK, EDIT_NOTEBOOK, EXECUTE_NOTEBOOK steps; ReadNotebook, EditNotebook, ExecuteNotebook RPCs. [LS cortex_pb, extension_server_pb]
- [SRC-STR, med] Applets (generated mini web apps): COMPILE_APPLET, LINT_APPLET, INSTALL_APPLET_DEPENDENCIES, INSTALL_APPLET_PACKAGE steps; copy "applet" present. [LS]
- [SRC-STR, high] Web app deploy and dev server: DEPLOY_WEB_APP step (with a file-upload enum), DEPLOY_FIREBASE, SET_UP_FIREBASE, RESTART_DEV_SERVER steps. [LS]
- [SRC-STR, high] Cloud SQL tooling: CLOUD_SQL_EXECUTE_SQL, CLOUD_SQL_UPDATE_SCHEMA, SET_UP_CLOUD_SQL steps. [LS cortex_pb]

### 3.3 Research and web

- [SRC-STR, high] Web search: SEARCH_WEB step; GetWebSearchResults and GetWebSearchRedirect RPCs. [LS api_server_pb]
- [SRC-STR, high] Read a web page: READ_URL_CONTENT step, read_url_content tool; VIEW_CONTENT_CHUNK step. [LS]
- [SRC-STR, med] Background research with refresh: CORTEX_TRAJECTORY_TYPE_BACKGROUND_RESEARCH; ForceBackgroundResearchRefresh RPC; CortexResearchState. [LS]
- [SRC-STR, med] Deep-think trajectories: CORTEX_TRAJECTORY_TYPE_DEEP_THINK; "thinking" model variants. [LS]
- [SRC-STR, high] Connectors to external sources: KnowledgeBaseService ConnectKnowledgeBaseAccount, IngestGithubData, IngestGoogleDriveData, IngestJiraData, IngestSlackData, ForwardSlackPayload, GetKnowledgeBaseWebhookUrl, UpdateConnectorConfig. Connector types GITHUB, GITHUB_OAUTH, GOOGLE_DRIVE, JIRA, SLACK, EMAIL, CODEIUM. [LS opensearch_clients_pb, codeium_common_pb]
- [SRC-STR, high] Knowledge base search and ingestion jobs: KnowledgeBaseSearch, GetKnowledgeBaseJobStates, GetKnowledgeBaseScopeItems, CancelKnowledgeBaseJobs; steps for knowledge-base lookup and search (CortexStepLookupKnowledgeBase, CortexStepSearchKnowledgeBase). [LS]
- [SRC-STR, med] Knowledge artifacts and generation: KI_INSERTION, KNOWLEDGE_ARTIFACTS, KNOWLEDGE_GENERATION steps; CORTEX_TRAJECTORY_TYPE_KNOWLEDGE_GENERATION. [LS]
- [SRC-STR, low] Internal search and query tools (Google-internal; names only): INTERNAL_SEARCH and MQUERY steps; GetMQuery RPC. [LS]
- [SRC-STR, high] MCP OAuth for remote tool servers: CompleteMcpOAuth, DisconnectMcpOAuth; copy "Failed to get OAuth token". [LS]

### 3.4 Agent runtime

- [SRC-STR, high] Planner loop: PLANNER_RESPONSE and PLAN_INPUT steps; CortexPlanConfig, CortexPlanState, CascadePlannerConfig, CascadeConversationalPlannerConfig. [LS]
- [SRC-STR, high] Progress boundaries shown to the user: TASK_BOUNDARY step; task_boundary tool. [LS]
- [SRC-STR, high] Agent-to-user notifications: NOTIFY_USER step; notify_user tool. [LS]
- [SRC-STR, high] Tool calling and proposals: TOOL_CALL_PROPOSAL and TOOL_CALL_CHOICE steps, TRAJECTORY_CHOICE, AGENCY_TOOL_CALL, MCP_TOOL step, ChatToolCall. [LS]
- [SRC-STR, high] Deferred tool discovery: TOOL_SEARCH step; tool_search tool. [LS]
- [SRC-STR, high] Subagents: INVOKE_SUBAGENT step and invoke_subagent tool; manage_subagents (kill) tool. Prompt text names an orchestrator subagent and a pipeline conductor subagent; each worker runs in an isolated CitC clone. DOC-GS lists "Custom subagents". [LS; DOC-GS]
- [SRC-STR, high] Browser subagent: BROWSER_SUBAGENT step and browser_subagent tool; BrowserSubagentMode and BrowserSubagentContextConfig. [LS]
- [SRC-STR, high] Custom agents and agent configs: CustomAgentSpec, CustomAgentConfig, CustomAgentSystemPrompt, BuiltinAgentConfig, AgentConfig (AgentType, DefaultAgent), AgentConfigForSubagent; GetAllCustomAgentConfigs. [LS]
- [SRC-STR, high] Background tasks and commands: manage_task tool (list action), BackgroundCommand, BackgroundTask, BackgroundTasksUpdate with a fullyIdle flag. [LS]
- [SRC-STR, high] Per-tool background mode for MCP tools: MCP_TOOL_BACKGROUND_MODE_OFF and _ALWAYS; error text for unknown background_mode. [LS]
- [SRC-STR, high] Auto-run policies and decisions: AUTO_RUN_POLICY_{DISABLED, ENABLED, MODEL_DECIDES}; AUTO_RUN_DECISION_{DEFAULT, MODEL, SYSTEM, USER}_{ALLOW, DENY}; BrowserJsAutoRunPolicy; RunExtensionCodeAutoRunDecision; AutoFixLintsConfig; test auto-fix config (AUTO_RUN_TESTS_FIXES_*). [LS]
- [SRC-STR, high] Checkpoints and revert: CHECKPOINT step, CheckpointConfig, CheckpointStrategy, CortexStepCheckpoint; revert marker "cortex-snapshot-revert"; checkpoints record running background tasks. [LS]
- [SRC-STR, high] Prompt assembly: PromptSection, CumulativePromptConfig, UnifiedPromptComponents, RenderSystemPrompt; directory rules injected through DIRECTORY_RULES. [LS]
- [SRC-STR, high] Retries and error classification: GenerateFullResponseWithRetry, ModelAPI.ClassifyError, CortexErrorDetails, ErrorTrace. [LS]
- [SRC-STR, high] Lifecycle hooks: PRE_INVOCATION, POST_INVOCATION, PRE_TOOL_USE, POST_TOOL_USE, STOP, SessionStart; hook kinds CommandHook and PromptHook; HookInjectedStep, HookSystemMessage, HookEphemeralMessage, PreToolHookResult, StopHookDecision. [LS hooks_pb, framework/agent]
- [SRC-STR, med] Turn and conversation cost accounting: TurnCostSummary, ConversationCostSummary, CostDisplaySection, CostCategory, ModelPricingInfo. [LS jetski_cortex_pb]
- [SRC-STR, med] Code-review agent steps: START_CODE_REVIEW, PROPOSE_AI_COMMENTS, POST_PR_REVIEW, CRITIQUE, FINDINGS. [LS cider proto]
- [SRC-STR, low] Judge and eval infrastructure (internal; names only): LLM_JUDGE trajectory type, GOOGLE_TRAINING_JUDGE, model enums ending in _EVAL. [LS]
- [SRC-STR, high] Run telemetry: AnalyticsService RecordCortexTrajectory, RecordCortexTrajectoryStep, RecordCommandUsage; ApiServer RecordCascadeUsage. [LS]
- [SRC-STR, high] Diagnostics: DumpFlightRecorder and DumpPprof RPCs; CreateDebugBundle RPC. [LS]
- [SRC-STR, high] Agent step vocabulary (CORTEX_STEP_TYPE, 122 values). Groups: browser (about 25 BROWSER_* steps plus CLICK_BROWSER_PIXEL, CAPTURE_BROWSER_*, OPEN/LIST/READ_BROWSER_*); code (CODE_SEARCH, CODE_ACTION, CODE_ACKNOWLEDGEMENT, PROPOSE_CODE, FIND, FIND_ALL_REFERENCES, GREP_SEARCH, VIEW_FILE, VIEW_FILE_OUTLINE, VIEW_CODE_ITEM, FILE_CHANGE, LINT_DIFF); shell and terminal (RUN_COMMAND, SHELL_EXEC, SEND_COMMAND_INPUT, COMMAND_STATUS, READ_TERMINAL); files (WRITE_BLOB, MOVE, DELETE_DIRECTORY, LIST_DIRECTORY); git (GIT_COMMIT); memory and knowledge (MEMORY, RETRIEVE_MEMORY, BRAIN_UPDATE, KI_INSERTION, KNOWLEDGE_ARTIFACTS, KNOWLEDGE_GENERATION, DIRECTORY_RULES); subagents (INVOKE_SUBAGENT, BROWSER_SUBAGENT); planning (PLANNER_RESPONSE, PLAN_INPUT, TASK_BOUNDARY); interaction (ASK_QUESTION, NOTIFY_USER, USER_INPUT, SUGGESTED_RESPONSES); media and web (GENERATE_IMAGE, SEARCH_WEB, READ_URL_CONTENT); build and deploy (BLAZE_* Google-internal, COMPILE, DEPLOY_WEB_APP, DEPLOY_FIREBASE, RESTART_DEV_SERVER, CLOUD_SQL_*); control (CHECKPOINT, WAIT, FINISH, ERROR_MESSAGE, SYSTEM_MESSAGE, EPHEMERAL_MESSAGE, CONVERSATION_HISTORY). Full list from the binary is in the scratch file tmp/ls_cortex_step_types.txt. [LS cortex_pb]

### 3.5 Tools, plugins and extensibility

- [SRC-STR, high] MCP servers: McpServerSpec, McpServerTemplate (GetMcpServerTemplates), server statuses READY, PENDING, ERROR, DISABLED_BY_ADMIN; auth provider type GOOGLE_CREDENTIALS; copy "Internal MCP manager is disabled"; BlockOnMcpInit setting. DOC-GS lists "MCP" under Extend. [LS; DOC-GS]
- [SRC-STR, high] Plugin marketplace: GetAvailableCascadePlugins, GetCascadePluginById, InstallCascadePlugin, GetAllPlugins, DeletePlugin; CascadePluginTemplate with local and remote configs and command templates with variables; MarketplaceInstall and PluginUserConfig. UI RPCs OpenConfigurePluginsPage, OpenPluginPage, OpenPluginConfigModal. [LS cascade_plugins_pb, config_pb, extension_server_pb]
- [SRC-STR, high] Agent skills: GetAllSkills, DeleteSkill, GenerateSkillInstallationCL, SkillUserConfig. DOC-GS lists "Agent skills" and "Workflows to skills migration". [LS; DOC-GS]
- [SRC-STR, high] Workflows (reusable procedures): GetAllWorkflows, GetDefaultWorkflowTemplates, CopyBuiltinWorkflowToWorkspace, WorkflowSpec, CortexWorkflowState. DOC-GS lists "IDE workflows". [LS; DOC-GS]
- [SRC-STR, high] Rules: GetAllRules, DiscoverRulesForPaths, GenerateSyntheticRule (name only), CustomizationPath, CustomizationSourceConfig, CustomizationDiscoveryConfig; OpenAntigravityRulesFile. DOC-GS lists "Rules" under the Skills group. [LS; DOC-GS]
- [SRC-STR, high] Custom agent scripts: AgentScriptItem, AgentScriptCommandSpec, AgentScriptPythonSpec, GetAgentScripts. [LS]
- [SRC-STR, high] Sidecars (local processes with a web UI): SidecarConfig; SidecarUIConfig with FULL_PANE and AUX_PANE entry points; statuses RUNNING, STOPPED, ERRORED, BACKOFF; actions CREATE, UPDATE, UPDATE_ARGS, RESTART, ENABLE, DISABLE, DELETE, SET_PROJECT, SET_ALL_PROJECTS; env ANTIGRAVITY_SIDECAR_WEB_PORT and ANTIGRAVITY_SIDECAR_UI_TOKEN; sidecar UI injected into an iframe or webview. [LS; SHELL customScheme.js]
- [SRC-STR, med] Python SDK: sample import "from google.antigravity import Agent, LocalAgentConfig, CapabilitiesConfig"; AgentConfig and CapabilitiesConfig types present. [LS]
- [DOC-GS, high] Antigravity CLI: install the client, then launch a TUI inside a project. LS strings "agy --help" and "agy CLI"; a subcommand string "agents kill" also present. [DOC-GS; LS]
- [DOC-GS, high] Slash command /browser: controls Chrome debugging behaviour (DOC wording paraphrased). [DOC-GS]
- [SRC-STR, high] IDE and extension bridge (for IDE plugins): ExtensionServerService methods such as ReadTerminal, ShowTerminal, OpenTerminal, SendTerminalInput, TerminateCommand, GetDefinition, FindAllReferences, GetLintErrors, OpenDiffZones, InsertCodeAtCursor, OpenFilePointer, SaveDocument, OpenVirtualFile, ExecuteCommand, RunExtensionCode, PlaySound, FocusIDEWindow. DOC-GS lists IDE extensions for VS Code, JetBrains, Visual Studio, Zed and Xcode. [LS; DOC-GS]
- [SRC-STR, med] Secrets via the IDE bridge: GetSecretValue and StoreSecretValue (extension_server_pb). Values not read. [LS]
- [SRC-STR, med] Remote connections: RemotingService ConnectRemote, DisconnectRemote, ListRemotes, ListConnectedRemotes, SaveRemote, RespondToPrompt; RemoteDescriptor and RemoteConnectionPrompt. Transport not verified. [LS remoting]
- [SRC-STR, high] Bundled Chrome DevTools MCP exposed to the LS: CHROME_DEVTOOLS_MCP_JS env; GetChromeDevtoolsMcpUrl and LaunchBrowserReverseProxy (extension server). [SHELL languageServer.js; LS]
- [CDM, high] WebMCP: list and execute tools that web pages expose (list_webmcp_tools, execute_webmcp_tool). [CDM tools/webmcp.js]
- [CDM, high] Agent-side Chrome extension management: install_extension, list_extensions, reload_extension, trigger_extension_action, uninstall_extension. [CDM tools/extensions.js]

### 3.6 Computer and environment control

- [CDM, high] Browser automation toolset (chrome-devtools-mcp 1.1.0). Navigation: navigate, navigate_page, new_page, close_page, select_page, list_pages, get_tab_id, wait_for. Input: click, click_at, drag, fill, fill_form, hover, press_key, type_text, upload_file, handle_dialog. Inspection: evaluate_script, take_snapshot (accessibility tree), take_screenshot, screenshot, get_console_message, list_network_requests, get_network_request. Emulation: emulate, resize_page. Performance: performance_start_trace, performance_stop_trace, performance_analyze_insight, lighthouse_audit. Memory: take_heapsnapshot with summary, details, class-node and retainer queries. Recording: screencast_start, screencast_stop. [CDM tools/*.js]
- [SRC-STR, high] Browser step types for the agent: BROWSER_CLICK_ELEMENT, BROWSER_DRAG_PIXEL_TO_PIXEL, BROWSER_GET_DOM, BROWSER_GET_NETWORK_REQUEST, BROWSER_INPUT, BROWSER_LIST_NETWORK_REQUESTS, BROWSER_MOUSE_DOWN, BROWSER_MOUSE_UP, BROWSER_MOUSE_WHEEL, BROWSER_MOVE_MOUSE, BROWSER_PRESS_KEY, BROWSER_REFRESH_PAGE, BROWSER_RESIZE_WINDOW, BROWSER_SCROLL (+DOWN, UP), BROWSER_SELECT_OPTION, EXECUTE_BROWSER_JAVASCRIPT, OPEN_BROWSER_URL, READ_BROWSER_PAGE, LIST_BROWSER_PAGES, CAPTURE_BROWSER_CONSOLE_LOGS, CAPTURE_BROWSER_SCREENSHOT, CLICK_BROWSER_PIXEL. [LS cortex_pb]
- [SRC-STR, high] Browser access policy: URL allowlist and denylist (DOC-GS "Allowlist and denylist"); RPCs AddToBrowserWhitelist and GetAllBrowserWhitelistedUrls; copy "allowlisted URL". [LS; DOC-GS]
- [SRC-STR, high] Separate browser profile and browser onboarding: DOC-GS "Separate Chrome profile"; RPCs CheckDevToolsActivePort and GetBrowserOnboardingPort; steps CascadeOpenBrowserSetup and CascadeConfirmBrowserSetup. [LS; DOC-GS]
- [SRC-STR, high] Browser verification modes: BrowserVerifyCompletenessMode, BrowserVerifyScreenshotsMode, BrowserToolSetMode, BrowserStateDiffingConfig (diff-based state reporting). [LS]
- [SRC-STR, high] Terminal sessions: CreateTerminal, ShowTerminal, OpenTerminal, ReadTerminal, SendTerminalInput, TerminateCommand, CloseTerminal, CheckTerminalShellSupport; steps RUN_COMMAND, SHELL_EXEC, SEND_COMMAND_INPUT, COMMAND_STATUS, READ_TERMINAL; BackgroundCommand. [LS]
- [SRC-STR, high] Terminal sandbox with a per-project toggle. Standard mode: workspace read and write, no network. Bypass mode: isolation disabled, network and full filesystem access. DOC-GS lists "Terminal sandbox", "Execution modes" and "Permissions". [LS prompt text; DOC-GS]
- [SRC-STR, med] Windows sandbox helper "exebox": AppContainer profile create and delete, loopback SID handling; env BOX_SBOXSERVE. [LS]
- [SRC-STR, high] File access controls: FilePermissionInteraction with BlockReason, FileAccessPermission, PermissionScope. [LS]
- [SRC-STR, high] Filesystem tools: VIEW_FILE, VIEW_FILE_OUTLINE, VIEW_CODE_ITEM, VIEW_CONTENT_CHUNK, WRITE_BLOB, replace_file_content and multi_replace_file_content, CreateDirectory, DeleteFileOrDirectory, MOVE, LIST_DIRECTORY, find_by_name, grep_search, CLIPBOARD. [LS]
- [SRC-STR, high] Git and version control: GIT_COMMIT step; GenerateCommitMessage, CheckoutCommit, DiscardFileChanges RPCs; VCS types (5 values); VcsFileChange. [LS]
- [SRC-STR, high] Git worktrees for parallel work: CreateWorktree, CheckoutWorktree, DeleteWorktree; copy describes sharing a repository similar to a git worktree. [LS]
- [SRC-STR, low] Google-internal VCS and build (names only): CogCommit, CogSync, CogUpload, FigCommit, FigSync, CreateCitcWorkspace, BLAZE_BUILD_TARGETS, BLAZE_TEST_TARGETS. [LS]
- [SRC-STR, med] Hosting and account connectors: ConnectNetlifyAccount, DisconnectNetlifyAccount, GetNetlifyAccountStatus, ConnectGithubAccount, DisconnectGithubAccount. [LS seat_management_pb]
- [SRC-STR, low] Hybrid and self-hosted deployment RPCs (names only): CreateHybridDeploymentInternal, GetHybridDeploymentsInternal, CheckHybridDeploymentStatus, GetDeploymentConfig. [LS api_server_pb]
- [SRC-STR, low] Computer-use model enum GOOGLE_GEMINI_COMPUTER_USE_EXPERIMENTAL (name only). [LS]

### 3.7 Coding capability

- [SRC-STR, high] Code edits as reviewable diffs: diff_action_pb (DiffBlock, UnifiedDiff), OpenDiffZones, AcknowledgeCascadeCodeEdit, CODE_ACKNOWLEDGEMENT step, TrajectoryFileDiffsUpdate, sed-style edit specs (ActionSpecSed). [LS]
- [SRC-STR, high] AI code review: START_CODE_REVIEW, PROPOSE_AI_COMMENTS, POST_PR_REVIEW steps; StartCodeReviewStep and ProposeAICommentsStep. [LS cider proto]
- [SRC-STR, high] Test and fix loops: AUTO_RUN_TESTS_FIXES config, BLAZE_TEST_TARGETS, BUILD_CLEANER, COMPILE step, AutoFixLintsConfig. [LS]
- [SRC-STR, high] Lint and diagnostics: GetLintErrors (extension server), CodeDiagnostic, LINT_DIFF step. [LS]
- [SRC-STR, high] Symbol navigation: GetDefinition, FindAllReferences, VIEW_CODE_ITEM, FIND_ALL_REFERENCES step. [LS]
- [SRC-STR, high] Codebase indexing and embeddings: IndexService and IndexManagementService (AddRepository, EnableIndexing, DisableIndexing, RetryIndexing, GetRemoteIndexStats); GetEmbeddings and GetEmbeddingsForCodeContextItems; GetNearestCCIsFromEmbedding; CodeIndexService GraphSearch and HybridSearch. [LS]
- [SRC-STR, high] Tab completion: DOC-GS "Tab completion"; SUPERCOMPLETE trajectory type; GetCompletionExamples and RecordCompletions RPCs; internal tab models GOOGLE_GEMINI_INTERNAL_TAB_FLASH_LITE and _TAB_JUMP_FLASH_LITE. [LS; DOC-GS]
- [SRC-STR, high] Commit message generation: GenerateCommitMessage RPC; COMMIT_MESSAGE_GENERATION trajectory type. [LS]
- [SRC-STR, high] Code context and retrieval: CodeContextItem, RetrievedCodeContextItemMetadata; context change events for active document, cursor position, open documents, pinned context and pinned guidelines. [LS context_module_pb]
- [SRC-STR, med] Code search: CODE_SEARCH step, CodeSearchToolConfig, CodeSearchResults. [LS]

### 3.8 Memory, learning and personalization

- [SRC-STR, high] Memory: CortexMemory with scopes GLOBAL, LOCAL, PROJECT and ALL, plus sources and triggers; MEMORY step; RETRIEVE_MEMORY step; DeleteCascadeMemory RPC. [LS cortex_pb]
- [SRC-STR, high] Brain: learned entries (BrainEntry, BrainEntryType), BRAIN_UPDATE step, BrainUpdateTrigger, BrainEntryDelta and summary; copy "brain" present. [LS]
- [SRC-STR, med] Personalisation model enum (MODEL_PERSONALIZATION, name only). [LS]
- [SRC-STR, high] Feedback loop for the agent: MANAGER_FEEDBACK step with ManagerFeedbackStatus; PROPOSAL_FEEDBACK step; ProvideFeedback RPC. [LS]
- [SRC-STR, high] Experiments and feature flags: ExperimentConfig and ExperimentKey messages; GetUnleashContextFields RPC. [LS]
- [SRC-STR, med] Preapproved permissions and allow-always grants: GrantPreapproval, RejectPreapproval, GetPreapprovals; AllowAlwaysConfig; PermissionGrantsConfig. [LS]
- [SRC-STR, med] Session forking and checkpoints as the restore path: ForkConversation and checkpoint revert. [LS]

### 3.9 Models and routing

- [SRC-STR, high] Gemini family as present in binary strings (noisy; exact IDs need confirmation): gemini-3-pro-preview, gemini-3-pro-image, gemini-3-flash-preview, gemini-3.1-pro (with -preview, -low, -low-thinking, -preview-custom-tools variants), gemini-3.1-flash-lite (and -preview), gemini-3.1-flash-image (and -preview), gemini-3.5-flash-low and -extra-low, gemini-3.6-flash and gemini-3.7-flash (low, medium, high variants), gemini-2.5-pro, gemini-2.5-flash, gemini-2.5-flash-lite, gemini-2.5-flash-image-preview, gemini-2.5-pro-windsurf (and -debug). [LS]
- [SRC-STR, high] Claude family: claude-sonnet-4-6, claude-sonnet-4-5, claude-sonnet-4, claude-opus-4-8, claude-opus-4-6, claude-opus-4-5, claude-opus-4; enums CLAUDE_3_5_HAIKU, CLAUDE_3_7_SONNET, CLAUDE_3_OPUS, CLAUDE_4_5_HAIKU, CLAUDE_4_5_SONNET, CLAUDE_4_OPUS, CLAUDE_4_SONNET. [LS]
- [SRC-STR, high] Bring-your-own-key Claude routes: enums such as MODEL_CLAUDE_4_OPUS_BYOK and MODEL_CLAUDE_3_7_SONNET_BYOK, plus variants routed through OpenRouter and Databricks. [LS]
- [SRC-STR, med] Other hosted and open models in enums: GPT-OSS-120B (medium), GPT-4.1, o4-mini (referenced as a third-party web-search model), DeepSeek R1 (fast, slow, internal) and V3 (0324 internal), Llama 3.1 (8B, 70B, 405B; Hermes-3 variants), Llama 3.3 70B, Qwen 2.5 (7B, 32B, 72B; R1 variant), Mistral 7B. Availability in the product is unverified. [LS]
- [SRC-STR, med] Windsurf-lineage aliases: MODEL_ALIAS_SWE_1 and MODEL_ALIAS_SWE_1_LITE; gemini-2.5-pro-windsurf. [LS]
- [SRC-STR, med] Model aliases and automatic selection: MODEL_ALIAS_AUTO, _RECOMMENDED, _CASCADE_BASE; CORTEX_TRAJECTORY_TYPE_AUTO_CASCADE and AUTO_CASCADE_MANAGER; RPCs GetCascadeModelConfigs, GetCommandModelConfigs, GetModelInfos, GetModelStatuses. [LS]
- [SRC-STR, high] Provider families: API_PROVIDER_GOOGLE_GEMINI, GOOGLE_VERTEX, GOOGLE_EVERGREEN (internal), ANTHROPIC_VERTEX, OPENAI_VERTEX, INTERNAL. [LS]
- [SRC-STR, med] Compatible-endpoint enums: ANTHROPIC_COMPATIBLE, AZURE_COMPATIBLE, BEDROCK_COMPATIBLE, OPENAI_COMPATIBLE; GOOGLE_GEMINI_INTERNAL_BYOM (bring-your-own-model); CustomModels topic in unified state sync. [LS]
- [SRC-STR, med] Model capacity and status: MODEL_CAPACITY_EXHAUSTED enum (overload signal); ModelStatuses RPC. [LS]
- [SRC-STR, high] Pricing and credits: CreditUsageSummary, ConsumedCredits, CortexStepCreditReason, AddExtraFlexCredits (internal variant), GetTeamCreditEntries; ModelPricingInfo and pricing-type enum. [LS]
- [SRC-STR, med] Reasoning and thinking controls: model variants (-low, -medium, -high, -thinking); ANTHROPIC_ANTIGRAVITY_RESEARCH_THINKING enum; DEEP_THINK trajectory type. [LS]
- [SRC-STR, high] Prompt caching controls: CacheRequestOptions and CacheBreakpointMetadata. [LS]
- [SRC-STR, med] Structured tool schemas: ToolConverter, JSON-schema strings, "invalid JSON schema" error. [LS]
- [SRC-STR, med] Model access and training-privacy controls: access statuses UNRESTRICTED, RESTRICTED_FOR_TRAINING, RESTRICTED_GENERALLY; MODEL_TRAINING_PRIVACY_CONTROL; GOOGLE_GEMINI_TRAINING_POLICY; GEMINI_API_LOGGING_RESTRICTIONS_STORE. [LS]
- [SRC-STR, med] Embedding and speech models: Hugging Face text-embedding enum; TTS voice setting; transcription RPC. [LS]

### 3.10 Transparency, observability and control

- [SRC-STR, high] Step-level trace UI: StepRenderInfo, AccumulatedItemRenderInfo, AccumulatedSections, ActionDebugInfo, generator and executor metadata. [LS cortex_pb]
- [SRC-STR, high] Tool-call inspection: TOOL_CALL_PROPOSAL and TOOL_CALL_CHOICE steps, ChatToolCall, ActionStatus, ActionResult. [LS]
- [SRC-STR, high] Cost and usage meters per turn and per conversation: TurnCostSummary, ConversationCostSummary, CostCategory, CostDisplaySection, ModelUsageStats. [LS]
- [SRC-STR, low] Year-in-review usage stats: GetWrapped2024 (name only). [LS]
- [SHELL, high] Logs: LS log file (language_server.log) and electron-log file; the logs:electron IPC reads the shell log. [SHELL paths.js, ipcHandlers.js]
- [SRC-STR, high] Support bundle and profiling: CreateDebugBundle, DumpFlightRecorder, DumpPprof RPCs. [LS]
- [SRC-STR, high] Undo and revert: checkpoint revert marker "cortex-snapshot-revert"; DiscardFileChanges RPC. [LS]
- [SRC-STR, high] Explainable auto-run: AutoRunDecision records the deciding source (default, model, system or user). [LS]
- [SHELL, high] Settings surface: runInBackground, keepComputerAwake, autoCheckForUpdates stored in app_storage.json. [SHELL services/settingsService.js]
- [SRC-STR, med] Team and user configuration records: UserConfig, ConversationGroupConfig, PluginUserConfig, SkillUserConfig, UserStatus, PlanInfo. [LS config_pb, codeium_common_pb]

### 3.11 Safety, permissions and governance

- [SRC-STR, high] Permission presets: AgentPermissionPreset with PERMISSION_PRESET values (5). [LS]
- [SRC-STR, high] Command risk assessor: CommandAssessorConfig with built-in rule text and examples (remote execution through ssh, nc, scp, rsync is called out). [LS]
- [SRC-STR, high] Sandbox policy field per command: BypassSandbox true or false (standard versus bypass), as in the agent prompt text. [LS]
- [SRC-STR, high] Admin controls and org policy: FetchAdminControls, TeamOrganizationalControls (Get and Delete), MCP status DISABLED_BY_ADMIN, PermissionGrantsConfig, IsConversationSharingBlocked. [LS]
- [SRC-STR, high] SSO and OIDC: GetSSOProvider, GetTeamOidcProviders, GetOidcAuthorizationUrl, ExchangeOidcCode, DeleteOidcProvider. [LS api_server_pb]
- [SRC-STR, high] Seats, roles and domains: CreateRole, DeleteRole, AddUserRole, BulkUpdateUserRoles, GetRolesForUser, AddTeamDomain, ListTeamDomains, BulkEditUserApprovals. [LS seat_management_pb]
- [SRC-STR, high] Team API keys and provider-key management: CreateTeamApiSecret, DeleteTeamApiSecret, GetAllTeamApiSecrets, GetSetUserApiProviderKeys, DeleteUserApiProviderKey, MigrateApiKey (names only; no values read). [LS seat_management_pb]
- [SRC-STR, med] Allowlist administration: GetAllowlist, InsertAllowlist, DeleteAllowlist, GetUserAllowlist. [LS api_server_pb]
- [SRC-STR, med] Data-training controls: MODEL_TRAINING_PRIVACY_CONTROL, RESTRICTED_FOR_TRAINING access status, GEMINI_API_LOGGING_RESTRICTIONS_STORE (see 3.9). [LS]
- [SHELL, high] Electron hardening observed: contextIsolation on, nodeIntegration off, devtools off in packaged builds, window.open denied and routed to the system browser, external URL opening limited to https, http and antigravity-ide schemes, certificate override only for 127.0.0.1 and localhost. REA observed 33 IPC sender-validation sites. [SHELL utils.js, ipcHandlers.js, languageServer.js; REA-JS]
- [SHELL, high] Host bridge hardening: loopback-only listener, peer-address check, Host-header check (DNS rebinding), bearer token from crypto.randomBytes(32), constant-time comparison. [SHELL hostBridgeServer.js]
- [SHELL, med] Observed weak points (INF, medium confidence): the LS CSRF token and host-bridge token are passed as command-line arguments (--csrf_token, --host_bridge_token) and can show in process listings; the LS inherits the full shell environment (shell-env), so secrets in that environment reach agent-run processes; the Electron fuse wire matches Electron defaults, so EnableEmbeddedAsarIntegrityValidation (index 4) and OnlyLoadAppFromAsar (index 5) are off even though an ELECTRONASAR integrity resource is embedded in Antigravity.exe. [SHELL languageServer.js; REA-PE; DOC-EL]
- [SHELL, med] Fuse wire read from the exe sentinel: bytes 1,0,1,1,0,0,0,1,1 for indices 0 to 8 (RunAsNode on, EnableCookieEncryption off, EnableNodeOptionsEnvironmentVariable on, EnableNodeCliInspectArguments on, EmbeddedAsarIntegrityValidation off, OnlyLoadAppFromAsar off, LoadBrowserProcessSpecificV8Snapshot off, GrantFileProtocolExtraPrivileges on, WasmTrapHandlers on). Read by me with od; index mapping from DOC-EL. [SRC-STR; DOC-EL]
- [SHELL, high] Auto-update channel and checks: electron-updater, channel latest-x64-win, first check after 10 s, hourly thereafter, packaged builds install on quit. Signature verification path not reviewed. [SHELL updater.js]
- [CDM, high] Browser MCP telemetry and update checks, with opt-outs CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS and CHROME_DEVTOOLS_MCP_NO_UPDATE_CHECKS. Whether Antigravity sets these opt-outs is unverified. [CDM telemetry/ClearcutLogger.js, bin/check-latest-version.js]
- [SRC-STR, med] Internal corporate hostnames appear in LS strings (hygiene observation; no values recorded here). [LS]

### 3.12 UI/UX and product design

- [SHELL, high] Window chrome: hidden title bar; Windows title-bar overlay (height 30); macOS traffic-light position; theme-aware background (#131313 dark, #FAFAFA light); theme modes DARK, LIGHT, INHERIT. [SHELL utils.js]
- [SHELL, high] Splash screen "Loading Antigravity" with animated dots. [SHELL loadingOverlay.js]
- [SHELL, high] System tray with running-agent count, Open and Quit. [SHELL tray.js; ipcHandlers.js agent:update-active-count]
- [SHELL, high] macOS dock menu with New Window. [SHELL main.js]
- [SHELL, high] Menus and shortcuts: New Window (CmdOrCtrl+Shift+N), Help > Docs, Cmd/Ctrl+Q with quit confirmation. [SHELL menu.js, keybindings.js, main.js]
- [SHELL, high] First-run wizard "Welcome to the new Antigravity!" with an option to download the IDE in the background. [SHELL ideInstall/wizard*.js]
- [DOC-GS, high] Agent side panel and editor features (tab completion) in docs navigation. [DOC-GS]
- [SHELL, med] Localisation shell: 55 Chromium locales shipped (af through zh-TW). App UI localisation not verified. [SHELL locales/]
- [SHELL, high] Desktop only: no mobile target found in the install. [SHELL; directory listing]
- [SHELL, med] Long-running agent support: runInBackground keeps the app alive after the last window closes; keepComputerAwake blocks display sleep. [SHELL settingsService.js, utils.js]
- [SRC-STR, med] Quit confirmation when agents are active: "There may be agents or background tasks running." [SHELL main.js]

### 3.13 Platform architecture

- [SHELL+LS, high] Process model: the Electron main process spawns the Go LS as a child with stdio pipes; the LS reports its port; the renderer loads https://127.0.0.1:<port>/ from the LS; the LS calls back to the shell over a loopback Connect server (host bridge). [SHELL main.js, languageServer.js, hostBridgeServer.js]
- [SHELL, high] Headless mode (ELECTRON_OZONE_PLATFORM_HINT=headless): no windows and no tray; stdin forwarded to the LS; LS started with --headless; Linux auto-install relaunches headless. [SHELL main.js, updater.js]
- [SHELL, high] Developer overrides: DEV_URL, JETSKI_LS_PORT, CODEIUM_LANGUAGE_SERVER_BIN. [SHELL main.js, languageServer.js]
- [SHELL+LS, high] Wire protocols: Connect-style RPC with protobuf (exa.* packages) over HTTP(S); streaming reactive updates; custom scheme plugin://; deep link antigravity://; antigravity-ide:// for opening the IDE. [SHELL preload.js, customScheme.js, ipcHandlers.js; LS proto names]
- [LS, high] Auth: Google OAuth through accounts.google.com; the LS prints the authorization URL and waits for a pasted code in headless or terminal mode; refresh-token and id-token handling present in strings. [SHELL languageServer.js AUTH_URL_PATTERN; LS strings]
- [SHELL+LS, high] Storage: app_storage.json (userData); LS state under the app data dir (ls_<id>.json naming in strings); DevToolsActivePort in userData; logs in the app logs dir; settings file path ~/.gemini/config/config.json (read by code for theme; not read here). [SHELL paths.js, utils.js; LS strings]
- [LS, low] Embedded database: SQLite-style PRAGMA and upsert strings present (INF: SQLite-backed trajectory and battle-mode storage). Verify before relying on it. [LS]
- [LS, high] Cloud endpoints referenced: generativelanguage.googleapis.com (Gemini API), daily-cloudcode-pa.googleapis.com (Cloud Code), oauth2, sts and iamcredentials googleapis hosts, play.googleapis.com, safebrowsing.googleapis.com; the IDE auto-updater Cloud Run host; the docs site. [SHELL languageServer.js, ideInstall/constants.js; LS strings]
- [SHELL, high] Update and distribution: electron-updater with a private-GitHub provider module present; resources/app-update.yml feed; Squirrel-aware exe; IDE installer downloaded from a metadata endpoint. [SHELL updater.js, ideInstall; REA-PE; app-update.yml]
- [SHELL, med] Process isolation: 0 utility processes and 0 native addon bindings in the shell (REA summary); the LS is a separate process. [REA-JS]
- [SRC-STR, med] Crash handling: LS stderr buffered and scanned for trigger phrases (panic, fatal error, unexpected fault address, runtime:); stack extracted and logged. [SHELL languageServer.js]

## 4. Extension points (non-candidate summary)

* Plugin marketplace: cascade plugins with install, configure and per-command templates [LS].
* Sidecars: local processes with a web UI, lifecycle actions, proxied through plugin:// [LS; SHELL].
* MCP servers: stdio and remote, OAuth, templates, admin enable or disable, background mode per tool [LS].
* Skills, workflows and rules: workspace-discovered customization paths and reusable procedures [LS; DOC-GS].
* Custom agents and agent scripts (command and Python specs) [LS].
* Lifecycle hooks: pre and post invocation, pre and post tool use, stop; command or prompt kinds [LS].
* Python SDK google.antigravity (Agent, LocalAgentConfig, CapabilitiesConfig) [LS string].
* CLI agy (TUI and subcommands) [DOC-GS; LS strings].
* IDE extension bridge (ExtensionServerService) for VS Code, JetBrains, Visual Studio, Zed and Xcode [LS; DOC-GS].
* Connectors (GitHub, Google Drive, Jira, Slack, email) with webhook URL [LS].
* Deep links antigravity:// and the plugin:// scheme [SHELL].
* Chrome DevTools MCP as a bundled server, WebMCP tool discovery, agent-installable Chrome extensions [CDM].

## 5. Data and protocol hints

* Storage: app_storage.json (shell key-value); ~/.gemini/antigravity data dir (names only); LS logs; DevToolsActivePort; ls_<id>.json naming; ~/.gemini/config/config.json (path only, not read) [SHELL; LS].
* Wire formats: protobuf over Connect-style RPC (exa.* packages, 2,989 names); JSON for update metadata and settings; WebM for browser recordings [SHELL; LS; WEB].
* Auth: Google OAuth (accounts.google.com; pasted code in headless or terminal mode); LS CSRF token (--csrf_token, ANTIGRAVITY_CSRF_TOKEN); host-bridge bearer token; sidecar UI token (ANTIGRAVITY_SIDECAR_UI_TOKEN); MCP OAuth tokens; team API keys held server-side (names only) [SHELL; LS].
* Sync and state: reactive streams from LS to UI (StreamReactiveUpdates, StreamAgentStateUpdates); unified state sync topics (UnifiedStateSync, SubscribeToUnifiedStateSyncTopic, PushUnifiedStateSyncUpdate); share links for conversations; checkpoints for revert [LS].
* Telemetry: AnalyticsService and api-server Record* RPCs; Clearcut telemetry in the bundled MCP with opt-out env vars [LS; CDM].
* Data residency and retention signals: model access and training-privacy enums; logging-restrictions store. No explicit zero-data-retention identifier was found (ZDR grep hits were binary noise) [LS].

## 6. Blockers, limits and absences

* Editor not present: Antigravity IDE (VS Code-based) is not installed on this PC and was not inventoried.
* UI bundle not extracted: the LS has no PE resources [REA-PE]. The UI likely ships as an embedded archive (305 zip local-file signatures; strings "Failed to open index.html in zip" and "Serving UI bundle from filesystem"). A later agent should locate and extract it to verify chat rendering, keyboard shortcuts, i18n and accessibility claims.
* LS RPC names are noisy in grep output (adjacent strings concatenated), so the method list is not clean. Deep-dive should use REA open_binary with list_strings or the protobuf raw descriptors, respecting the shared active-target session.
* REA trace limits: channel-kind traces exceeded the MCP response budget. Retained evidence for the deep-dive: ev_8521f7d29b3a2f5b7d8305f29306b5307011e3d8fc797f84b59d7981095362c8 (seed "channel contains :") and ev_2100a887dcb48c4c34799cee63a632fbd77a90d39833eae1534ebfd6f738cb88 (seed "updater:check-for-updates"). Parent JS analysis: ev_07f3b30797b1ab05de8887e6262b25e5409b2d05896c0e698bd33b9ef65a7ddc (session-local). Coverage partial (semantic-family budgets). Exact or substring API seeds matched nothing ("powerSaveBlocker"); that is not evidence of absence.
* REA open_binary, search_strings and list_strings were not used on the 153 MB Go LS, to avoid colliding with the shared REA active target. Strings were read with grep -a instead (read-only).
* Public docs: antigravity.google/docs redirects to getting-started, which is index-like. Feature detail for the agent manager, browser, artifacts, rules and MCP is label-only [DOC-GS].
* Names are not behaviour. Battle Mode semantics, sidecar lifecycle, hybrid deployment, connectors, eval and judge, and Google-internal steps (BLAZE_*, CLOUD_SQL_*, COG and FIG commands, CitC) are identifier-level evidence only.
* Model IDs come from binary strings concatenated with neighbours; exact IDs and per-plan availability are unknown.
* Not verified: default auto-run policy; sandbox enforcement beyond prompt text and exebox identifiers; telemetry defaults; updater signature checks; whether SQLite is actually used; per-command BypassSandbox semantics.
* Absences (string search only; not proof): no Ollama or LM Studio identifiers (0 hits); no seatbelt, landlock or bubblewrap identifiers (0 hits); no office-document generation identifiers (pptx, docx and xlsx hits are unrelated); no explicit prompt-injection defence phrase (0 hits); no desktop-wide mouse and keyboard tool identifiers beyond browser-scoped steps; no chart library identifiers.
* Not read by design: ~/.gemini/* (config.json and the antigravity, antigravity-ide and antigravity-backup dirs), AppData/Roaming/Antigravity user data, cookies and token stores. No secret values were printed or recorded.
* Electron fuse index mapping relies on DOC-EL; the fuse bytes were read by me from the exe (medium-high confidence).

## 7. REA usage log (tools actually run)

* analyze_javascript_application (resources/app.asar, format asar, detail summary, integrity_policy record-and-continue): ran. Statistics: 782 relevant files, 682 parsed, 561 modules, 2 browser windows, 7 explicit web preferences, 2 preload entry points, 11 context-bridge APIs, 61 IPC operations (33 literal channels, 28 main handlers, 28 renderer transmissions), 33 sender-validation observations, 0 utility processes, 0 native addon bindings. Coverage partial. Output exceeded the reply budget and was read with jq from the saved file.
* inspect_analysis_view (modules page, offset 0, limit 40, on the retained parent analysis): ran. Confirmed the bundled chrome-devtools-mcp internals and the telemetry module.
* trace_application_feature: channel exact "" (no match); channel contains ":" (over budget, retained); api outgoing exact "" (no match); api contains "powerSaveBlocker" (no match); channel exact "updater:check-for-updates" (over budget, retained).
* inspect_pe_resources: language_server.exe (0 resources, complete); Antigravity.exe (ELECTRONASAR INTEGRITY resource, RT_VERSION, RT_ICON and RT_GROUP_ICON, RT_MANIFEST, cursor groups); webm_encoder.exe (0 resources, complete).
* Not used: open_binary, search_strings, list_strings, binary_overview, inspect_artifact, and the analyze_function family (see section 6).

## 8. Files produced in scratch (for the deep-dive agent)

* scratchpad/tmp/ls_exa_names.txt (2,989 exa.* names), ls_cortex_step_types.txt (122 step types), ls_enums_misc.txt, ls_model_enums.txt (120 model enum names), ls_urls.txt (URL hosts), ls_rpc_methods.txt (noisy RPC list, 632 entries before cleanup).
* scratchpad/extract/antigravity/dist/ (shell source) and the REA parent analysis above.
