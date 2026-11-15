T3 CODE 0.0.35 (DESKTOP BUILD) - CAPABILITY INVENTORY (AUDIT 3, scout)
=====================================================================
Purpose: input for the deep-dive agent. Breadth and accurate pointers over depth.
File type: plain text on purpose (see 0.2).
Scope: installed program files, the pre-extracted copy, and the source maps shipped inside the app.
The app was NOT launched. No AppData/Roaming/t3code or AppData/Local/t3code-updater reads. No credential values were read (env var and settings NAMES only).

PROVENANCE TAGS
REA = observed through REA analysis of a shipped artifact (evidence IDs in 0.3)
SRC = read in shipped source: app-owned TypeScript recovered from .map sourcesContent, or string constants in shipped JS or native binaries
DOC = in-source documentation or help text (CLI descriptions, module comments, prompt text)
INF = my inference; basis stated. Confidence in brackets where it matters (high / med / low)

PATH ALIASES
SCR = C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-AppData-Roaming-Claude-scratch-workspaces-5e23d905-dd91-4518-8ff4-252efcbe0520-b5ff8b3a-437c-4499-9a7f-469de994f319-scratch-2026-10-12-69a4ee/1e4c8548-5aa8-429f-9d17-693211694c40/scratchpad
INSTALL = C:/Users/ImaFernandesDaCosta/AppData/Local/Programs/t3code
SM-client = SCR/tmp/t3code/recovered/client/src (SPA sources from the SPA bundle map; repo path apps/web/src is INF med)
SM-server = SCR/tmp/t3code/recovered/server/src and SCR/tmp/t3code/recovered/server/packages (apps/server, packages/*)
SM-desktop = SCR/tmp/t3code/recovered/desktop/src and SCR/tmp/t3code/recovered/desktop/packages (apps/desktop, packages/*)
EXT-desktop = SCR/extract/t3code/apps/desktop/dist-electron (main.cjs, preload.cjs, maps)
EXT-server = SCR/tmp/t3code/server-x/apps/server/dist (bin.mjs server bundle, client/ SPA, maps)
ASAR listings = SCR/tmp/t3code/app_asar_entries.tsv (desktop) and SCR/tmp/t3code/server_asar_entries.tsv (server)

==========================================================
0. METHOD, REA USAGE, DEVIATIONS
==========================================================
0.1 Method
- Read-only parse of both asar headers (no extraction into INSTALL): app.asar 10,595 entries; server.asar 1,210 entries.
- Extracted server.asar JS and maps into a NEW scratch folder (EXT-server). Native binaries (.dll .exe .so .pdb .node .dat .bin) were skipped.
- Recovered app-owned TypeScript from sourcesContent of four shipped maps: SPA bundle 498 files, server bundle 440 files, desktop main 161 files, desktop preload 2 files (1,100 total). Third-party node_modules were not recovered.
- Grep and read of the recovered trees and bundles for NAMES: IPC channels, RPC methods, settings keys, CLI commands, orchestration events, env var names, routes, keyboard command ids, enum literals.
- Keyword presence counts across the three trees. Presence only; several keywords are generic (hooks = React hooks; review = PR review; test = unit tests; fallback = generic; injection = dependency injection; stripe = stripElement UI variable).
- Never launched the app or any binary from it; never ran project code, tests or builds.

0.2 Deviations from the requested output
- The file is .txt, not .md. The agent-level rule forbids report .md files; this file is an input for the next agent. Its path is in the StructuredOutput "file" field.
- Two heavy REA analyses instead of one: the app ships two separate archives (desktop app.asar and server.asar). Each got one heavy run.

0.3 REA tools used (path- or evidence-based; the shared active-target session was NOT used)
- analyze_javascript_application, DESKTOP app.asar (format asar, detail summary, integrity record-and-continue, heap 2048 MB, timeout 600000 ms). Completed, but the MCP response exceeded the 10 MB transport budget. Retained: ev_e559df801f0124f651832bfbc447d96a21d9d77007e211064b4e7880fd7545b4. A second record of the same app.asar digest is ev_f15195178ac9979ffe3ee4621adb2028cf405b29507c85a53ebff19edd75af56. app.asar sha256 96d2e68de4f9ecc245522a4227a6e69f965edffd18c2b3b5f3577d848729cf0e. About 16,700 limitations recorded.
- get_evidence_bundle (detail summary, filter operation=analyze_javascript_application): 119 retained records in the shared session, 21 match; includes the two desktop records.
- inspect_analysis_view on the desktop record (summary view; modules page offset 0 limit 25): both exceeded the transport budget (ev_f4aabce424d30140cb8136cb9eeb6dc62516135e1d6fd77e09ada6e15288a966; ev_72d095dbc6a799a5830565e023d8d34d1ca44007eeae7023023ec1fc716f556c). No content obtained.
- trace_application_feature on the desktop record, exact string seed "desktop.ipc.preview.pickElement": ran, 0 matches, graph partial. Retained as ev_0612f0cd798abe9f1806e3f1517e06e1bab4387b4cf0436d312c0c4ffc23d10c. "No match" is not evidence of absence. All desktop IPC facts below therefore come from SRC, not REA.
- analyze_javascript_application, SERVER bundle directory (format directory, detail summary, heap 4096 MB, timeout 600000 ms). Completed; the summary was saved by the harness to a tool-result file and read with node. Parent evidence ev_91f9bb20250cfa305309a418f454fd8d7facd6a547e9df9c49c2410f81dd68c3 (wrapper ev_528db95f6b0429eeeb668c73857c6a5fdc245cf628d35990c2d46aa5bc8110a1). Stats: 800 relevant files; 400 parsed JS files; 97,037,149 text bytes read; 3,462 findings; 0 modules recovered; 0 Electron entities; 774 limitations. Coverage partial in all 12 semantic families (retained relations: call-flow 1,523; data-flow 14,781; closure 1,534; object-flow 842; promise-ownership 122; event 47; child-process 16; boundary 26; configuration 2; resource-lifecycle 11; timer 7; request 0).
- trace_application_feature on the server parent evidence, string "app-server" (contains): matched, but the MCP response exceeded the budget. Retained as ev_6f5c925689ecbfc56f947940eb6afc28ccf1642af122daa9ceab0bdadb79cbde; not readable through MCP and not exported (export_evidence_bundle would dump the whole shared session).
- inspect_pe_resources on resource-monitor/t3-resource-monitor.exe: complete. PE32+ AMD64 (machine 0x8664), 390,592 bytes, 0 resource entries (no version resource, no icon). Evidence ev_974d9254a1d03539860ab3999ef3e83ef16d0ac17dc0cd8651bb94c4b329281c.
- Not used: open_binary, binary_session, search_strings, list_strings (shared target session); binary_overview (no native target); export_evidence_bundle.

==========================================================
1. IDENTITY
==========================================================
1.1 Product and build [SRC high]
- Product: T3 Code, installed as "T3 Code (Alpha).exe" (uninstaller in INSTALL). Package name t3code; version 0.0.35; buildVersion 0.0.35; build commit t3codeCommitHash f925d6394218 (root package.json; the build can override it with env T3CODE_COMMIT_HASH). Package description: "T3 Code desktop build".
- Publisher and feed: publisherName "T3 Tools Inc"; package author "T3 Tools"; updater config (INSTALL/resources/app-update.yml): provider github, owner pingdotgg, repo t3code, releaseType release, updaterCacheDirName t3code-updater (cache dir NOT read).
- Release channels: latest and nightly (SM-desktop updates/updateChannels.ts).
- Update controls: env T3CODE_DISABLE_AUTO_UPDATE; mock update server env T3CODE_DESKTOP_MOCK_UPDATES and T3CODE_DESKTOP_MOCK_UPDATE_SERVER_PORT (dev and test hooks).
- Runtime versions: Electron 41.5.0 and Chromium 146.0.7680.216 (user-agent strings found by read-only grep of the installed exe).

1.2 Process model [SRC high]
- Electron main (app.asar: apps/desktop/dist-electron/main.cjs) runs DesktopBackendManager and DesktopBackendPool. These start a Node server per backend instance, running the server entry bin.mjs from server.asar (apps/server/dist/bin.mjs). The Electron binary is the Node runtime (process.execPath). The window loads the server-served SPA (main.cjs: window.loadURL(applicationUrl)). A "connecting" splash window is shown first.
- Desktop wrapper does not contain the UI: app.asar has apps/desktop plus node_modules only. The SPA is inside server.asar at apps/server/dist/client (787 files, about 71.6 MB including maps).
- SSH remote hosts start a server with the serve command via a bootstrap script in main.cjs; WSL has a server-tree module (desktop/src/wsl). Whether WSL runs bin.mjs is INF med.
- Bootstrap: an envelope is delivered via stdin on the WSL path (SRC); the server CLI takes --bootstrap-fd to read one-time bootstrap secrets from a file descriptor (SRC/DOC high).

1.3 Monorepo layout [SRC high unless marked]
- pnpm workspace (packageManager pnpm@11.10.0). Apps: desktop, server, and the SPA (repo path apps/web is INF med).
- Packages: contracts, shared, client-runtime, effect-acp, effect-codex-app-server, ssh, tailscale.
- pnpm overrides and patches also name Expo and React Native packages (@clerk/expo, expo-modules-jsi, react-native-*). A mobile app exists in the monorepo but is not part of this build [INF med; basis pnpm-workspace.yaml].
- Overrides also pin @effect/atom-react, @effect/platform-node, vite (to @voidzero-dev/vite-plus-core 0.2.2) and Clerk packages.

1.4 Tech stack [SRC high unless marked]
- TypeScript everywhere. Effect 4.0.0-beta.103 is the service, layer, schema and config framework on desktop and server (Layer, Context.Service, Schema, Config, Option, Effect RPC).
- SPA (client): React 19.2.6; TanStack Router 1.17.x, Query 5.100.14, Pacer 0.19.x; Base UI 1.5.0; lucide-react 0.564.0; Lexical 0.41.0 (composer); react-markdown 10.1.0 with remark-gfm 4.0.1, remark-breaks 4.0.0, rehype-raw 7.0.0, rehype-sanitize 6.0.0; Shiki 4.2.0; @pierre/diffs 1.3.0-beta.10 and @pierre/trees 1.0.0-beta.4 (diff and file-tree UI); @legendapp/list 3.3.5 (virtual list, patched); @dnd-kit; @formkit/auto-animate 0.9.0; Clerk (clerk-js 6.30.1, react 6.14.7); jszip 3.10.1; heic-to (lazy chunk); Ghostty VT WASM (terminal renderer).
- Build tooling: Vite (override to @voidzero-dev/vite-plus-core 0.2.2).
- Server: Node runtime; the SQLite driver switches at run time between Bun (@effect/sql-sqlite-bun) and Node (NodeSqliteClient) [SRC high]. @effect/platform-node 4.0.0; node-pty 1.1.0 (ConPTY and OpenConsole on Windows); @ff-labs/fff-node 0.9.4 (native file-search library libfff_c, Windows fff_c.dll); @yuuang/ffi-rs 1.3.7 (FFI); @opencode-ai/sdk 1.15.13; jose 6.2.2 (JWT and DPoP); ws 8.21.0; msgpackr 2.0.4. The Claude Agent SDK is referenced in the server bundle map (version and code not extracted) [SRC med].
- Desktop: electron-updater 6.8.9; electron-store 8.2.0; conf 10.2.0; playwright-core 1.60.0; react-grab 0.1.50 with @react-grab/cli 0.1.50; bippy 0.6.1; @clerk/electron 0.0.37 and @clerk/electron-passkeys 0.0.3; undici 8.10.0.
- Storage: SQLite (state.sqlite) with 43 migration files; JSON settings and keybindings files; a secrets directory.
- Telemetry: PostHog (anonymous, env-gated) and OTLP traces and metrics exporters.

1.5 Size facts [SRC high]
- app.asar 91.6 MB, 10,595 entries. node_modules: effect 35.2 MB, @clerk 16.6 MB, playwright-core 10.6 MB, undici 1.7 MB, core-js 1.3 MB. apps/desktop: 20 files, 5.1 MB including maps.
- server.asar 130.5 MB, 1,210 entries. apps/server: 809 files, 99.4 MB including maps (dist/client 787 files, 71.6 MB). node-pty about 35 MB (mostly PDB files and prebuilds). @ff-labs native binaries 17.8 MB.

==========================================================
2. DIRECTORY / MODULE MAP (top 2-3 levels; one line each)
==========================================================
2.1 apps/desktop/src (SM-desktop; Electron main, about 72 files) [SRC high]
- main.ts, preload.ts: entry points; preload is the bridge whose channels are listed in ipc/channels.ts
- app/: DesktopApp, DesktopLifecycle, DesktopShutdown, DesktopState and paths, DesktopConfig (env reads), DesktopClerk, DesktopObservability, DesktopLinuxUrlHandler, DesktopAppIdentity, DesktopAssets, DesktopEnvironment, DesktopConnectionCatalogStore
- backend/: DesktopBackendManager (spawn and restart server instances), DesktopBackendPool (primary and others), DesktopBackendConfiguration, DesktopLocalEnvironmentAuth, DesktopServerExposure (local-only or network-accessible; LAN; Tailscale), tailscaleEndpointProvider, DesktopNetworkInterfaces
- electron/: thin wrappers over Electron: ElectronApp, ElectronWindow, ElectronMenu, ElectronDialog, ElectronShell, ElectronProtocol (custom protocol), ElectronPowerMonitor, ElectronTheme, ElectronSafeStorage, ElectronUpdater
- ipc/: DesktopIpc, DesktopIpcHandlers, channels.ts (every desktop:* IPC channel name)
- preview/: Manager (embedded browser tabs; webviews created with sandbox true), BrowserSession, GuestProtocol, PlaywrightInjectedRuntime (automation runtime), PickedElementPayload (element picker), FaviconCapture, PreviewKeyboard, WebviewPreferences (react-grab and bippy hook injection)
- settings/: DesktopAppSettings, DesktopClientSettings, DesktopSavedEnvironments (saved environment list)
- ssh/: DesktopSshEnvironment (discover hosts, ensure and disconnect environment, descriptor, session state, WebSocket tickets), DesktopSshPasswordPrompts
- wsl/: DesktopWslBackend, DesktopWslEnvironment, DesktopWslServerTree, wslPathParsing
- updates/: DesktopUpdates, updateChannels (latest, nightly), updateMachine, releaseNotes
- window/: DesktopWindow, DesktopApplicationMenu, QuitHold (hold-to-quit)
- telemetry/: DesktopTelemetryPublisher
- linuxSecretStorage.ts: Linux secret storage

2.2 packages/ (SM-desktop/packages and SM-server/packages) [SRC high]
- contracts/: wire types shared by all apps (rpc, ipc, orchestration, settings, provider, providerInstance, providerRuntime, preview, previewAutomation, terminal, sourceControl, pullRequest, git, vcs, filesystem, project, t3ProjectFile, usage, review, relay, relayClient, remoteAccess, resourceTelemetry, desktopBootstrap, environment, environmentHttp, keybindings, model, auth, assets, background, editor, server, baseSchemas)
- shared/: cross-cutting helpers (path, git, sourceControl, keybindings, preview, previewViewport, searchRanking, projectScripts, t3ProjectFile, toolActivity, cliArgs, claudeCompaction, semver, qrCode, devProxy, logging, observability, connectAuth, relayAuth, relayUrl, serverSettings, hostProcess, Net, advertisedEndpoint, backgroundActivitySettings, oauthScope, httpReadiness, httpObservability, filePreview)
- ssh/ (desktop only): command, auth, errors (SSH client wrapper)
- tailscale/: Tailscale CLI wrapper
- effect-acp/ (server only): Agent Client Protocol client over stdio with generated schema
- effect-codex-app-server/ (server only): Codex app-server client (stdio transport, protocol, generated schema)
- client-runtime/ (SPA and desktop): operations/projects, state/* (auth, filesystem, git, sourceControl, use-atom-query-runner), providerSkills, markdownImages

2.3 apps/server/src (SM-server; Node server, about 345 files) [SRC high]
- bin.ts (CLI root), entrypoint.ts, config.ts (paths and env reads), cli/ (auth, config, connect, invocation, pair, project, server, service, servicePreflight, triage, triagePrompt)
- ws.ts (WebSocket RPC endpoint), http.ts, httpCors.ts, httpResponseErrorGuard.ts, startupAccess.ts, serverRuntimeState.ts, serverRuntimeStartup.ts, serverActivation.ts, serverLifecycleEvents.ts, serverSettings.ts, keybindings.ts
- orchestration/: event-sourced core. decider.ts (command to event), projector.ts (event to read model), commandInvariants.ts, Normalizer.ts, Schemas.ts, Services/ (OrchestrationEngine, OrchestrationReactor, ProjectionPipeline, ProjectionSnapshotQuery), Layers/, ThreadBackgroundLiveness.ts, ThreadPlanProgress.ts, threadDetailCursor.ts, threadTitles.ts, ActivityPayloadProjection.ts, workflowScriptQuery.ts, runtimeLayer.ts
- persistence/: Layers/Sqlite.ts and NodeSqliteClient.ts (driver switch), Migrations (43 files), Services/Projection* (threads, messages, turns, activities, sessions, pending approvals, proposed plans, checkpoints, projects, state), OrchestrationEventStore, OrchestrationCommandReceipts, ProviderSessionRuntime, AuthSessions, AuthPairingLinks
- provider/: ProviderService, ProviderRegistry, ProviderAdapterRegistry, ProviderInstanceRegistry (Live and Hydration), ProviderSessionDirectory, ProviderSessionReaper, ProviderEventLoggers, EventNdjsonLogger, providerMaintenance (plus Runner and CommandCoordinator), providerSnapshot, providerUpdateSettings, makeManagedServerProvider, builtInDrivers.ts (Codex, Claude, Cursor, Grok, OpenCode), Drivers/ (ClaudeDriver, ClaudeExecutable, ClaudeHome, ClaudeSkills, CodexDriver, CodexHomeLayout, CursorDriver, GrokDriver, OpenCodeDriver), Layers/ (ClaudeAdapter, ClaudeProvider, CodexAdapter, CodexProvider, CodexSessionRuntime, CursorAdapter, GrokAdapter, OpenCodeAdapter, codexLaunchArgs), acp/ (AcpSessionRuntime, AcpCoreRuntimeEvents, AcpRuntimeModel, AcpAdapterSupport, CursorAcpExtension, CursorAcpSupport, XAiAcpExtension, GrokAcpSupport), opencodeRuntime.ts, CodexDeveloperInstructions.ts, model-manifest.json (recovered copy is 0 bytes; not read)
- textGeneration/: commit message and change-request title and body generation through provider models (ClaudeTextGeneration, CodexTextGeneration, CursorTextGeneration, GrokTextGeneration, OpenCodeTextGeneration, TextGenerationPrompts, TextGenerationPresets)
- git/: GitManager, GitWorkflowService (stacked actions), remoteRefs, Utils
- vcs/: VcsDriver (+Registry), GitVcsDriver (+Core), VcsProcess, VcsStatusBroadcaster, VcsProvisioningService, VcsProjectConfig
- checkpointing/: CheckpointStore (hidden git-ref checkpoints), CheckpointDiffQuery, Diffs, Utils, Errors
- review/: ReviewService (diff preview and diff file contents)
- sourceControl/: SourceControlProvider (+Registry, Discovery, RepositoryService), GitHub (GitHubCli, gitHubAuthStatus, GraphQL budget), GitLab (GitLabCli, gitLabAuthStatus), Azure DevOps (AzureDevOpsCli), Bitbucket (BitbucketApi), PrTemplateDetection, SourceControlRateLimit
- pullRequest/: PullRequestService, PullRequestProviderRegistry, per-host providers (GitHub, GitLab, Azure DevOps, Bitbucket), pullRequestChecks, JSON mappers, http.ts
- mcp/: McpHttpServer (route /mcp), McpSessionRegistry, McpProviderSession, McpInvocationContext, PreviewAutomationBroker, toolkits/preview (tools.ts, handlers.ts)
- preview/: Manager (in-memory preview sessions keyed by threadId and tabId), PortScanner (lsof on macOS and Linux)
- terminal/: Manager (TerminalManager; thread-scoped node-pty sessions)
- workspace/: WorkspacePaths, WorkspaceEntries, WorkspaceFileSystem (mutations), WorkspaceSearchIndex (fff-backed)
- project/: RepositoryIdentityResolver, T3ProjectFileLoader (t3.json), ProjectFaviconResolver, ProjectSetupScriptRunner
- assets/: AssetAccess, AttachmentUpload, attachmentStore, attachmentPaths, imageMime
- usage/: UsageService, usageTranscriptReader (reads provider transcript files), usagePricing, usageAggregation, usageScanCache, usageTranscripts
- resourceTelemetry/ and diagnostics/: ResourceMonitorBinary (runs the bundled resource-monitor exe), NativeTelemetryClient, ResourceTelemetryHistory, ProcessDiagnostics, ProcessResourceMonitor, TraceDiagnostics
- observability/: Metrics, RpcInstrumentation, BrowserTraceCollector, Attributes, Layers/
- telemetry/: AnalyticsService (PostHog, env-gated), Identify
- background/: BackgroundPolicy (pause rules), HostPowerMonitor
- cloud/: T3 Connect and managed relay: CliTokenManager, CliState, ManagedEndpointRuntime, bootService, serviceLauncherClient, serviceProtocol, selfUpdate, cliAuthHtml, relayTracing, traceRelayRequest, pinnedRuntime, environmentKeys, publicConfig, config, http
- auth/: EnvironmentAuth (+Policy), RpcAuthorization, PairingGrantStore, SessionStore, ServerSecretStore, dpop.ts, http.ts, utils.ts
- environment/: ServerEnvironment (+Label), RemoteOpenTargets (advertised SSH host names)
- process/: externalLauncher (open browser or editor apps), processRunner.ts
- os-jank.ts, pathExpansion.ts, atomicWrite.ts: OS workarounds and file helpers

2.4 apps/web/src (SM-client; SPA, about 451 files; repo path inferred, INF med) [SRC high]
- routes/ (TanStack file routes): __root; _chat with $environmentId/$threadId, draft/$draftId, index, pull-requests; projects.$projectKey; usage; settings (general, appearance, providers, integrations, source-control, connections, keybindings, diagnostics, archived); pair; connect and connect_.callback
- components/: chat/ (57 files: composer, model picker, file, terminal and skill chips, activity rows); settings/ (45); preview/ (26); pullRequest/ (23); ui/ (35, Base UI wrappers); cloud/ (5); clerk/ (8); auth/ (2); desktop/ (1: SSH password prompt); files/ (3: file picker); search/ (2: content search); sidebar/ (4); usage/ (3); about 70 top-level files (ChatView, ChatMarkdown, Sidebar, CommandPalette, GitActionsControl, BranchToolbar, DiffPanelShell, AgentsPanel, RightPanelTabs, RightPanelSheet, ThreadTerminalDrawer, ProviderUpdate*, QuitHoldOverlay, ServerUpdateAction, PullRequestThreadDialog)
- browser/ (about 25): ElectronBrowserHost, HostedBrowserWebview, BrowserSurfaceSlot, BrowserDeviceToolbar, browserRecording, browserPointerStore, browserHistoryStore, browserFaviconStore, annotationTheme, browserTargetResolver, webviewCrashRecovery, openFileInPreview
- terminal/ghostty/: Ghostty VT WASM runtime, renderer, surface, key codes, fonts
- state/: atoms for auth, git, sourceControl (and actions), usage, filesystem, desktopNetworkAccess, desktopSshHosts, desktopWslState
- cloud/ and environments/primary/: managed auth, relay link state, connect onboarding, CLI auth connect
- connection/onboarding.ts; hooks/ (useLiveRefresh, useTurnDiffSummaries, useCustomThemes, useActiveProjectTarget, useT3ProjectFileScripts, useResizableWidth, useCommitOnBlur)
- lib/ (diffRendering, syntaxHighlighting, imageCompression (HEIC), previewFocus, lruCache, terminalCloseConfirm, terminalCloseShortcut, favicon, remoteOpen, wslPaths, resourceTelemetryState)
- observability/clientTracing.ts; editorPreferences.ts; appearanceFonts.ts; appearanceContrast.ts; openVsxThemes.ts; vscodeThemeImport.ts; projectScripts.ts; composer-logic.ts; composer-editor-mentions.ts; markdown-*.ts; terminal-links.ts; pendingUserInput.ts; proposedPlan.ts; planAgentSelectionHeal.tsx; diffPanelStore.ts; rightPanelLayout.ts; previewMiniPlayerStore.ts

2.5 Routing and navigation notes [SRC high]
- Thread URLs carry an environment id and a thread id (_chat.$environmentId.$threadId), so one UI can address several servers (local, SSH, WSL, relay).
- Draft threads have their own route (_chat.draft.$draftId).
- Pull requests have a top-level route (_chat.pull-requests); project settings live under projects.$projectKey.

==========================================================
3. FEATURE CANDIDATES (grouped by taxonomy)
==========================================================
Format: "- [TAG conf] capability. Pointer." Lines marked "NOT OBSERVED" are checks with no match (absence is SRC-based and only as strong as the keyword search).

3.1 CATEGORY 1 - CHAT EXPERIENCE
- [SRC high] Multi-thread chat with an event-sourced timeline; assistant text streams as delta events, then a complete event. Pointer: SM-server contracts/orchestration.ts; orchestration/decider.ts; orchestration/projector.ts; events thread.message.assistant.delta and .complete.
- [SRC high] Streaming render mode: buffered or streaming, with a legacy token-streaming switch. Pointer: SM-server packages/contracts/src/settings.ts (enableLegacyTokenStreaming; enum buffered|streaming).
- [SRC high] Markdown rendering: react-markdown with GFM, GitHub-style alerts, list-indentation and clipboard helpers, link handling, sanitized HTML. Pointer: SM-client components/ChatMarkdown.tsx; SM-client markdown-*.ts; markdown-github-alerts.ts.
- [SRC high] Code highlighting with Shiki (code block component). Pointer: SM-client lib/syntaxHighlighting.ts; ShikiCodeBlockProps.
- [SRC high] Diff rendering in chat and in the right panel (@pierre/diffs), with a diff worker pool and an ignore-whitespace toggle. Pointer: SM-client lib/diffRendering.ts; components/DiffPanelShell.tsx; components/DiffWorkerPoolProvider.tsx; setting diffIgnoreWhitespace.
- [SRC high] Per-turn diff summaries and whole-thread diffs. Pointer: RPC orchestration.getTurnDiff and orchestration.getFullThreadDiff; event thread.turn.diff.complete; SM-client hooks/useTurnDiffSummaries.ts.
- [SRC high] Checkpoint revert (rewind the workspace to an earlier turn): command thread.checkpoint.revert; events thread.revert.complete and thread.reverted. Pointer: SM-server checkpointing/CheckpointStore.ts (hidden git refs); orchestration/decider.ts.
- [SRC high] Plan mode: interaction mode enum default|plan; proposed plans (thread.proposed-plan.upsert; turn.proposed.delta and turn.proposed.completed); plan progress tracking. Pointer: SM-server orchestration/ThreadPlanProgress.ts; setting planModeEnabled; SM-client proposedPlan.ts; planAgentSelectionHeal.tsx.
- [SRC high] Approval requests in chat: command thread.approval.respond; decision enum accepted|rejected; status pending|resolved; event thread.approval-response-requested. Pointer: SM-server contracts/orchestration.ts; SM-client pendingApprovals UI.
- [SRC high] Structured user-input requests from the agent: command thread.user-input.respond; event thread.user-input-response-requested. Pointer: SM-client pendingUserInput.ts; SM-server contracts/orchestration.ts.
- [SRC high] Thread lifecycle and organization: create, archive and unarchive, delete (with confirmation setting), pin, unpin and reorder, snooze and unsnooze, settle and unsettle, metadata update, title regeneration (thread.title.regeneration.complete), next and previous thread navigation. Pointer: contracts/orchestration.ts; SM-client components/Sidebar.snooze.ts; route settings.archived.
- [SRC high] Auto-settle rules: after N days (sidebarAutoSettleAfterDays) and on merge (sidebarAutoSettleOnMerge). Pointer: contracts/settings.ts.
- [SRC high] Sidebar: project or thread grouping (sidebarProjectGroupingMode and per-project overrides), sort by updated_at, created_at or manual, thread preview count, legacy sidebar switch, status indicator style (artwork, pill or none). Pointer: contracts/settings.ts; SM-client components/Sidebar.tsx; components/LegacySidebar.tsx.
- [SRC high] Draft threads (route _chat.draft.$draftId) and a prompt stash (keybinding composer.stash; localStorage key t3code:prompt-stash). Pointer: SM-client routes/; keybindings; localStorage keys.
- [SRC high] Conversation search across threads: RPC orchestration.searchThreads; command palette results. Pointer: SM-client components/CommandPalette*.tsx; components/search/ProjectContentSearchDialog.tsx.
- [SRC high] Composer: Lexical rich-text editor; @-mention of project files (composer-editor-mentions.ts; components/chat/FileTagChip.tsx); inline chips for terminal output used as context (TerminalContextInlineChip.tsx; ComposerPendingTerminalContexts.tsx); skill chips (SkillInlineText.tsx); drag-and-drop of workspace files (workspaceFileDrop.ts); footer layout and placeholder logic. Pointer: SM-client components/ComposerPromptEditor.tsx; composer-logic.ts.
- [SRC high] Slash menu with optional provider skills (setting showSkillsInSlashMenu; SM-client client-runtime providerSkills.ts; SM-server provider/Drivers/ClaudeSkills.ts).
- [SRC high] Context compaction: auto-compact window setting (autoCompactWindow); Claude compact boundary events (claude/system/compact_boundary); compaction banner dismissal (localStorage key t3code:resume-compaction-dismissed); SM-server packages/shared/src/claudeCompaction.ts.
- [SRC high] Token usage per thread: event thread.token-usage.updated; usage meters in the UI. Pointer: SM-server contracts/orchestration.ts; SM-client components/usage/.
- [SRC high] Background liveness for a thread: ThreadBackgroundLiveness shows native background work (subagent fleets, workflow runs, monitor loops per its module comment) as sidebar status (working or monitoring). Pointer: SM-server orchestration/ThreadBackgroundLiveness.ts.
- [SRC med] Realtime thread events mirror Codex app-server thread/realtime notifications (started and itemAdded verified in code; closed, error and audio delta assumed from event names): thread.realtime.started, thread.realtime.item-added, thread.realtime.audio.delta, thread.realtime.closed, thread.realtime.error. Pointer: SM-server provider/Layers/CodexAdapter.ts around lines 1446-1470. No T3-owned microphone or speech pipeline was found.
- [SRC high] NOT OBSERVED: message edit-and-resend, regenerate response, and conversation branching or fork (0 identifiers; "regenerat" hits are theme-palette code only).
- [SRC high] NOT OBSERVED: shareable conversation links, transcript export, citations and source cards, math rendering (no KaTeX or MathJax), Mermaid diagrams.

3.2 CATEGORY 2 - MULTIMODAL AND CREATION
- [SRC high] Image attachments in the composer. Accepted MIME types: avif, bmp, gif, heic, heif, jpeg, png, svg+xml, tiff, webp. Upload endpoint /api/attachments/upload; uploaded files stored under stateDir/attachments; asset URLs via /api/assets and RPC assets.createUrl. Pointer: SM-server assets/AttachmentUpload.ts, assets/AssetAccess.ts, attachmentStore.ts, imageMime.ts.
- [SRC high] HEIC and HEIF images are converted or compressed in the browser before upload (heic-to, lazy chunk). Pointer: SM-client lib/imageCompression.ts.
- [SRC high] Browser preview screen recording with MediaRecorder (MP4 with H.264 if supported, else WebM VP9 or WebM). Desktop IPC: desktop:preview-recording-start, -stop, -save, -frame. Pointer: SM-client browser/browserRecording.ts; EXT-desktop preview IPC.
- [SRC high] Preview screenshots (desktop:preview-capture-screenshot), copy and reveal of captured artifacts (desktop:preview-copy-artifact, desktop:preview-reveal-artifact), favicon capture (preview/FaviconCapture.ts).
- [SRC high] Annotations on previews: annotation theme (desktop:preview-set-annotation-theme; SM-client browser/annotationTheme.ts) and pointer events (desktop:preview-pointer-event; browserPointerStore.ts).
- [SRC high] Device emulation for previews: device toolbar with rotation and viewport presets. Pointer: SM-client browser/BrowserDeviceToolbar.tsx; browser/ScreenRotationIcon.tsx; SM-server packages/shared previewViewport.
- [SRC med] Image attachments are sent to provider turns (INF: the provider adapters receive them; the exact path was not traced).
- [SRC high] NOT OBSERVED: image generation (0 identifiers); speech-to-text and text-to-speech and voice input (no transcribe, whisper or tts identifiers); video generation; document, slide and spreadsheet generation (PDF handling only as an attachment type, if at all).
- [SRC high] Charts: only usage charts (components/usage/UsageProviderChart.tsx); no agent-generated chart rendering was found.

3.3 CATEGORY 3 - RESEARCH AND WEB
- [SRC high] Built-in web preview browser (this is browsing, not web search). Desktop IPC covers tab create and close, navigate, back, forward, refresh, hard reload, zoom, DevTools, clear cache, clear cookies, color scheme, audio mute. Pointer: EXT-desktop channels; SM-desktop preview/Manager.ts.
- [SRC high] Web search is NOT a T3 feature. The string web_search appears as a tool-activity kind mapped from provider events: Claude (SM-server provider/Layers/ClaudeAdapter.ts around line 772), ACP (provider/acp/AcpCoreRuntimeEvents.ts around line 57; provider/acp/AcpRuntimeModel.ts around line 374).
- [SRC high] NOT OBSERVED: deep research mode; citation and source-card rendering.
- [SRC high] Connectors: T3 hosts an MCP server (category 5) and integrates source-control and pull-request hosts (category 7). The theme catalog calls https://open-vsx.org/api/ (SM-client openVsxThemes.ts).
- [SRC high] Public endpoints referenced by the app: open-vsx.org (theme search and download). The triage playbook URL (GitHub main branch) is a runtime fetch (see category 11).

3.4 CATEGORY 4 - AGENT RUNTIME
- [SRC high] Provider-agnostic turn engine: commands thread.turn.start and thread.turn.interrupt; events turn.started, turn.completed, turn.aborted; session commands thread.session.set and thread.session.stop. Pointer: SM-server provider/Services/ProviderService (cross-provider orchestration), provider/Layers/ProviderService.ts, orchestration/Services/OrchestrationReactor.ts.
- [SRC high] Reactors connect commands, providers and projections: ProviderCommandReactor, ProviderRuntimeIngestion, CheckpointReactor, ThreadDeletionReactor (service summaries in SM-server).
- [SRC high] Five provider adapters: Claude (Claude Agent SDK query API; the query options include model, effort, permission mode, resume, session id, setting sources, additional directories, partial messages, path to executable, settings JSON, extra args JSON); Codex (codex app-server over JSON-RPC on stdio via effect-codex-app-server); Cursor and Grok (ACP over stdio; Cursor launches "agent stdio"); OpenCode (launches "opencode serve" and talks through @opencode-ai/sdk). Pointer: SM-server provider/Layers/*Adapter.ts; provider/acp/*; provider/opencodeRuntime.ts.
- [SRC high] Session resume: Claude resume fields (claude.resume.session_id, thread_id, turn_count, session_at); Codex thread resume (provider/Layers/codexLaunchArgs.ts); ProviderSessionDirectory and ProviderSessionReaper (idle reaping; idleClientTtl setting).
- [SRC high] Subagents: Claude subagent type maps to a role (SM-server provider/Layers/ClaudeAdapter.ts around lines 235 and 1072); the Agents right-panel "fleet view" shows the native subagent fold, with one spawn-batch row per batch in the chat (SM-client components/AgentsPanel.tsx header comment).
- [SRC high] Workflow scripts: a read-only viewer of persisted Claude workflow scripts. Reads are limited to paths under the Claude projects folder (containment by realpath). RPC orchestration.getWorkflowScript. Pointer: SM-server orchestration/workflowScriptQuery.ts.
- [SRC high] Background task events: Claude task_progress and task_notification handled; ThreadBackgroundLiveness tracks background work (category 4 item above).
- [SRC high] Hooks: Claude hook events (hook_started, hook_output, hook_finished) are mapped to activity rows; git hook runs during commit are reported as kind hook_started (SM-server git/GitManager.ts around line 1659).
- [SRC high] Context management: auto-compact window; compact boundary; token usage events; Codex model-rerouting notice (Codex model/rerouted, mapped to the model.rerouted event; SM-server provider/Layers/CodexAdapter.ts around line 1339).
- [SRC high] Error recovery and timeouts: ProviderAdapterRequestError (timeouts), ProviderAdapterProcessError, ProviderAdapterSessionClosedError, a slow-RPC toast coordinator in the SPA, provider health refresh interval (providerHealthRefreshInterval).
- [SRC high] Interrupt and stop: turn interrupt (thread.turn.interrupt), session stop (thread.session.stop), and the requested-stop event.
- [SRC high] Runtime modes (permission and sandbox presets): full-access, auto-accept-edits, approval-required. Claude mapping: full-access to bypassPermissions; auto-accept-edits to acceptEdits (SM-server provider/Layers/ClaudeAdapter.ts around lines 4132-4287). Codex mapping: approval-required to sandbox read-only; auto-accept-edits to workspace-write; full-access to danger-full-access (SM-server provider/Layers/CodexSessionRuntime.ts around lines 495-517).
- [SRC high] Text generation as a service: commit messages and change-request title and body from a chosen model (TextGeneration service; per-provider text-generation files; settings textGenerationModelSelection, sourceControlWriterModelSelection, sourceControlWritingStyle).
- [SRC high] NOT OBSERVED: scheduling, cron and recurring automations (only an internal scheduleFlush name); evals and benchmarks; agent memory.

3.5 CATEGORY 5 - TOOLS, PLUGINS, EXTENSIBILITY
- [SRC high] MCP HTTP server at /mcp that exposes T3 tools to provider sessions (McpHttpServer, McpSessionRegistry, McpProviderSession, McpInvocationContext). One toolkit, preview, with 14 tools: preview_open, preview_navigate, preview_status, preview_snapshot, preview_click, preview_type, preview_press, preview_scroll, preview_wait_for, preview_evaluate, preview_resize, preview_set_appearance, preview_recording_start, preview_recording_stop. Gated by the setting enableAgentBrowserAccess. Pointer: SM-server mcp/toolkits/preview/tools.ts and handlers.ts.
- [SRC high] MCP elicitation approvals (the string mcp_elicitation_approval appears in the server; MCP servers can ask the user through the approval UI).
- [SRC high] Provider-side plugin events pass through: Claude plugin_install event (SM-server provider/Layers/ClaudeAdapter.ts around line 3456). No T3 plugin manifest format or marketplace was found.
- [SRC high] Skills: provider skills are discovered for the composer and the slash menu (ClaudeSkills.ts; client-runtime providerSkills.ts; setting showSkillsInSlashMenu).
- [SRC high] Project config file t3.json, checked into the repository: fields seen in the schema include scripts, runOnWorktreeCreate, previewUrl, autoOpenPreview, defaultThreadEnvMode, icon, iconPath. Loader is best-effort (missing or invalid means absent). Pointer: SM-server project/T3ProjectFileLoader.ts; SM-server/SM-desktop packages/contracts/src/t3ProjectFile.ts.
- [SRC high] Project scripts: a setup script runs in a terminal when a worktree is created; scripts have keybindings and an editor. Pointer: SM-server project/ProjectSetupScriptRunner.ts; packages/shared projectScripts.ts; SM-client components/projectScriptEditor.tsx; lib/projectScriptKeybindings.ts.
- [SRC high] Keybinding customization: file keybindings.json in stateDir; RPC server.upsertKeybinding and server.removeKeybinding; Settings > Keybindings page; keybinding update toast.
- [SRC high] Settings file settings.json in stateDir, with a settings search index (components/settings/settingsSearch.ts).
- [SRC high] Themes: custom theme editor (ThemeEditorPanel, ThemeColorPicker, themeInspector, ThemeWireframe, ThemePreviewCircles); VS Code theme import (vscodeThemeImport.ts; desktop:pick-theme-files); Open VSX search and install (openVsxThemes.ts; VSIX read with jszip).
- [SRC high] Custom and provider-instance configuration: multiple provider instances per driver (providerInstances; ProviderInstanceRegistryLive and Hydration); per-instance binary path, launch args, home path, shadow home path, environment variables; custom model list (customModels); add-instance wizard (AddProviderInstanceDialog, AddProviderInstanceWizardSteps); accent colors (ProviderAccentColorPicker).
- [SRC high] CLI front door: t3 start, serve, service, project, pair, auth, connect, config, triage (details in section 4).
- [SRC high] Remote client API: /ws RPC plus pairing tokens and scoped bearer sessions issued by the CLI; discovery at /.well-known/t3/environment (details in section 4).
- [SRC high] NOT OBSERVED: a plugin marketplace, plugin SDK for end users, webhooks (0 identifiers), user-defined tool registration outside MCP.

3.6 CATEGORY 6 - COMPUTER AND ENVIRONMENT CONTROL
- [SRC high] Terminal sessions per thread: RPC terminal.open, terminal.new, terminal.split, terminal.splitVertical, terminal.attach, terminal.write, terminal.resize, terminal.clear, terminal.restart, terminal.close, terminal.toggle. node-pty (ConPTY on Windows). Terminal renderer is Ghostty VT WASM. Terminal links can open in the preview (components/preview/openTerminalLinkInPreview.ts). Pointer: SM-server terminal/Manager.ts; SM-client terminal/ghostty/*; components/ThreadTerminalDrawer.tsx.
- [SRC high] Filesystem access from the UI and agent side: RPC projects.listEntries, projects.readFile, projects.writeFile, projects.searchEntries, projects.searchContents, filesystem.browse. Writes go through WorkspaceFileSystem and atomicWrite.ts. Note: the UI can write project files.
- [SRC high] Git and VCS control: RPC vcs.init, vcs.createRef, vcs.listRefs, vcs.switchRef, vcs.pull, vcs.refreshStatus, vcs.createWorktree, vcs.removeWorktree. Pointer: SM-server vcs/*; git/GitWorkflowService.ts.
- [SRC high] Stacked git actions: RPC git.runStackedAction with kinds commit, commit_push, push, pr, create_pr. Also git.resolvePullRequest and git.preparePullRequestThread. Pointer: SM-server git/GitWorkflowService.ts; contracts/git.ts.
- [SRC high] Worktree-based threads: environment mode enum local or worktree (defaultThreadEnvMode; newWorktreesStartFromOrigin; branch toolbar env-mode and environment selectors). Pointer: SM-client components/BranchToolbarEnvModeSelector.tsx; BranchToolbarEnvironmentSelector.tsx.
- [SRC high] Browser control for agents: preview tabs keyed by threadId and tabId. Automation over desktop IPC: desktop:preview-automation-status, -snapshot, -click, -type, -press, -scroll, -evaluate, -wait-for. Runtime: PlaywrightInjectedRuntime.ts on playwright-core. Preview webviews run with sandbox true (desktop preview/Manager.ts around line 2825). Pointer: SM-desktop preview/*; SM-server mcp/PreviewAutomationBroker.ts; RPC previewAutomation.connect, previewAutomation.respond, previewAutomation.focusHost.
- [SRC high] Element picking and React component grabbing in previews: desktop:preview-pick-element and -cancel-pick-element; PickedElementPayload.ts; react-grab and bippy hook injection (WebviewPreferences.ts). Picked elements can be sent as context (INF med, from the composer chips).
- [SRC high] Dev-server detection: PortScanner uses lsof -iTCP -sTCP:LISTEN on macOS and Linux (the Windows branch was not read). Also a dev proxy helper (packages/shared devProxy) and a per-project preview URL (t3.json previewUrl). Pointer: SM-server preview/PortScanner.ts; preview/Manager.ts.
- [SRC high] Open in external tools: externalLauncher (browser or editor), RPC editor.openFavorite (keybinding editor.openFavorite), editor probes (desktop:probe-remote-editors). Editor targets seen in code: Cursor (cursor://), Zed (zed://), VS Code remote (vscode://vscode-remote), JetBrains launchers (webstorm, rider, pycharm, idea), Antigravity, Explorer. Pointer: SM-server process/externalLauncher.ts; environment/RemoteOpenTargets.ts; SM-client components/JetBrainsIcons.tsx.
- [SRC high] Remote environments: SSH hosts (desktop:discover-ssh-hosts; ensure, disconnect and fetch-descriptor environment calls; session state; WebSocket tickets; password prompt with resolve call); WSL distros (desktop:get-wsl-state, set-wsl-distro, set-wsl-only, set-wsl-backend-enabled); LAN exposure (serverExposure mode local-only or network-accessible; env T3CODE_DESKTOP_LAN_ACCESS); Tailscale Serve (env T3CODE_TAILSCALE_SERVE and port; CLI runs tailscale serve --https and tailscale status). Pointer: SM-desktop ssh/*, wsl/*, backend/DesktopServerExposure.ts; SM-server packages/tailscale/src/tailscale.ts.
- [SRC high] Pairing and onboarding: /pair page (token in URL fragment); QR codes (packages/shared qrCode.ts; components/ui/qr-code.tsx); CLI pair and auth pairing create. Pointer: SM-client components/auth/PairingRouteSurface.tsx; routes/pair.tsx.
- [SRC high] T3 Connect (managed relay): CLI connect login, link, status, publish, unlink, logout. The publish command stops or starts sending agent activity to mobile clients. The relay client install dialog is in the SPA (components/cloud/RelayClientInstallDialog.tsx). The connect command reports "unavailable in builds without public configuration" (DOC). Pointer: SM-server cli/connect.ts; cloud/*; SM-client cloud/*.
- [SRC high] Process control: RPC server.signalProcess (treat as sensitive); server.getProcessDiagnostics; ProcessResourceMonitor with the bundled t3-resource-monitor.exe (ResourceMonitorBinary). Pointer: SM-server diagnostics/*; resourceTelemetry/*.
- [SRC high] Background and power policy: BackgroundPolicy with pauseWhenOnBattery, pauseWhenHostLocked, pauseWhenHostLowPower, pauseWhenClientLowPower, active and idle intervals; HostPowerMonitor; RPC server.reportHostPowerState; server.getBackgroundPolicy. Keep-awake: not verified.
- [SRC high] Self-update: RPC server.updateServer and server.updateServerWithProgress (cloud/selfUpdate.ts). Provider CLI update: RPC server.updateProvider using installer kinds brew, bun, bun-global, npm, npm-global, pnpm, pnpm-global, pipe (SM-server provider/providerMaintenance*.ts).
- [SRC high] Background service: t3 service install, update, uninstall, status. Supported only on Linux with systemd and macOS with launchd; the Windows status text says unavailable on this machine (DOC; SM-server cli/service.ts).
- [SRC high] Quit guard: Chrome-style hold-to-quit (desktop window/QuitHold.ts; SPA components/QuitHoldOverlay.tsx); double tap quits; confirmQuit setting.

3.7 CATEGORY 7 - CODING CAPABILITY
- [SRC high] Diff review service: RPC review.getDiffPreview and review.getDiffFileContents (SM-server review/ReviewService.ts: ReviewDiffPreview and ReviewDiffFileContents types).
- [SRC high] Diff UI: @pierre/diffs and @pierre/trees; worker pool; ignore-whitespace toggle; turn diffs; checkpoint diffs (checkpointing/CheckpointDiffQuery.ts).
- [SRC high] Commit and PR authoring: stacked actions (commit, push, PR); PR template detection (sourceControl/PrTemplateDetection.ts); setting followChangeRequestTemplates; writing style; model choice for writing.
- [SRC high] Pull request workspace: list with filters; detail panel with summary, timeline, checks popover, reviewer picker, reactions, markdown editor; create a thread from a PR (components/PullRequestThreadDialog.tsx; git.preparePullRequestThread; git.resolvePullRequest); top-level route _chat.pull-requests. Pointer: SM-client components/pullRequest/* (23 files).
- [SRC high] Source-control providers: GitHub (gh CLI for PR data plus GraphQL with a budget guard); GitLab (glab, merge requests); Azure DevOps (az CLI and PR JSON mappers); Bitbucket (REST API; env names T3CODE_BITBUCKET_EMAIL, T3CODE_BITBUCKET_API_TOKEN, T3CODE_BITBUCKET_ACCESS_TOKEN). Repository clone, lookup and publish: sourceControl.cloneRepository, sourceControl.lookupRepository, sourceControl.publishRepository. Discovery: server.discoverSourceControl.
- [SRC high] Codebase search: WorkspaceSearchIndex on the fff native library; searchRanking (packages/shared); RPC projects.searchEntries and projects.searchContents.
- [SRC high] Project file picker with @-mentions (components/files/ProjectFilePicker.tsx).
- [SRC high] Code edits are made by the provider agents (Claude, Codex, Cursor, Grok, OpenCode); T3 shows activity and diffs, and it can revert to a checkpoint.
- [SRC high] Test runs: no dedicated test-runner UI was found. Project scripts can hold arbitrary commands; script kinds were not verified.
- [SRC high] NOT OBSERVED: LSP or language-intelligence features (the lsp hits are noise), tab completion, an IDE plugin (T3 opens external IDEs instead), automated code-review bots or PR bots.

3.8 CATEGORY 8 - MEMORY, LEARNING, PERSONALIZATION
- [SRC high] NOT OBSERVED: a T3-owned long-term memory or user model. "memory" hits are process or resource memory telemetry.
- [SRC high] NOT OBSERVED as a T3 feature: custom instructions or personas. Provider-side developer instructions exist for Codex (SM-server provider/CodexDeveloperInstructions.ts).
- [SRC high] Favorites and preference sets: settings favorites, providerModelPreferences, customModels, sidebar grouping overrides, per-provider accent colors.
- [SRC high] Transcript-driven usage history: UsageService reads provider session files (category 10), so usage covers turns that ran outside T3.
- [SRC high] Appearance personalization: theme editor and imported themes; four font families (code, composer, sans, terminal) with separate sizes; fontSmoothing; glassOpacity; appearanceContrast; timestampFormat (locale, 12-hour or 24-hour); wordWrap.
- [SRC high] Keybinding personalization: keybindings.json plus the Settings page.
- [SRC high] NOT OBSERVED: learning from usage, skill creation or skill learning, self-improvement loops, knowledge bases.

3.9 CATEGORY 9 - MODELS AND ROUTING
- [SRC high] Five built-in provider drivers with display labels Claude, Codex, Cursor, Grok, OpenCode (SM-server provider/builtInDrivers.ts; SM-client components/settings/providerDriverMeta.ts). Each driver can have several provider instances (providerInstances; ProviderInstanceRegistryLive).
- [SRC high] Model picker: ProviderModelPicker, ModelPickerSidebar, ModelPickerContent, ModelListRow, modelPickerSearch, modelPickerModelHighlights, keyboard keys; favorites; custom models.
- [SRC med] Model identifiers present as code constants in the server bundle (not a live list). Claude: claude-opus-5, claude-opus-4-8, claude-opus-4-7, claude-opus-4-6, claude-opus-4-5, claude-sonnet-5, claude-sonnet-4-6, claude-haiku-4-5, plus dated IDs and thinking variants; claude-fable-5 also appears. Cursor: composer-1, composer-1.5, composer-2, cursor-agent. Codex and GPT: gpt-5.6-sol, gpt-5.6-luna, gpt-5.6-terra, gpt-5.4, gpt-5.3-codex, gpt-5.3-codex-spark, gpt-5.3-spark, gpt-5-codex. Grok: grok-build. Pointer: EXT-server bin.mjs string constants.
- [SRC high] Claude subscription tiers recognized for plan display: free, pro, max (5x and 20x), team, enterprise (usage module strings claudefreesubscription and related).
- [SRC high] Reasoning effort control: setting effort; Claude query option effort; Codex reasoning effort. Pointer: SM-server codexModelOptions.ts; ClaudeAdapter.ts.
- [SRC high] Codex service tier, exposed as an option value (getCodexServiceTierOptionValue). The "fast mode" label is INF med.
- [SRC high] Model rerouting: Codex notice mapped to model.rerouted. No T3-side fallback router was found (the reroute and fallback keyword hits were generic).
- [SRC high] Structured output: Codex turn output schema (outputSchemaJson) passed through; text generation uses structured prompts (TextGenerationPrompts.ts).
- [SRC high] NOT OBSERVED: prompt-caching controls; Ollama or LM Studio integration. OpenCode may run local models (INF low).
- [SRC high] Provider health and updates: providerHealthRefreshInterval; enableProviderUpdateChecks; dismissed update notifications (dismissedProviderUpdateNotificationKeys); ProviderUpdate* notification components; RPC server.refreshProviders and server.probe.
- [SRC high] Separate model choice for text generation: textGenerationModelSelection (titles and similar); sourceControlWriterModelSelection (commit and PR text).

3.10 CATEGORY 10 - TRANSPARENCY, OBSERVABILITY, CONTROL
- [SRC high] Provider event logs: EventNdjsonLogger and ProviderEventLoggers write NDJSON per provider session under the logs folder.
- [SRC high] Activity timeline: event thread.activity.append; ToolActivity (packages/shared toolActivity.ts): tool calls, hook output and task progress as rows.
- [SRC high] Usage and cost: RPC server.getUsageSummary; UsageService scans provider transcripts (Claude project transcripts; a sessions folder for Codex, INF med) with pricing tables (usagePricing.ts) and aggregation (usageAggregation.ts); usage page with charts. Its module comment says the approach follows ccusage.
- [SRC high] Process and resource diagnostics: server.getProcessDiagnostics; server.getProcessResourceHistory; server.getResourceTelemetryHistory; server.retryResourceTelemetry; Settings > Diagnostics page (components/settings/ResourceTelemetryDiagnostics.tsx).
- [SRC high] Trace diagnostics: RPC server.getTraceDiagnostics; OTLP trace and metric exporters (env T3CODE_OTLP_TRACES_URL, T3CODE_OTLP_EXPORT_INTERVAL_MS; OTLP metrics URL); client tracing (SM-client observability/clientTracing.ts); RPC instrumentation; BrowserTraceCollector; server OTLP ingest proxy /api/observability/v1/traces.
- [SRC high] Analytics: anonymous PostHog events (installation-scoped anonymous id in stateDir/anonymous-id); gated by env T3CODE_TELEMETRY_ENABLED (default not verified); desktop telemetry publisher and receiver.
- [SRC high] Config surface: settings.json (ServerSettings with schemaVersion), keybindings.json, the Settings UI with search and a Diagnostics page.
- [SRC high] Triage command (DOC): t3 triage writes context.md with machine facts (version, paths, server liveness), then launches claude or codex interactively with a built-in playbook. The playbook says to ask before writing to the database. Agent choice and model are flags. Pointer: SM-server cli/triage.ts; cli/triagePrompt.ts.
- [SRC high] Undo: only checkpoint revert (category 1); no general undo log found.
- [SRC high] NOT OBSERVED: benchmark or eval harness; audit log.

3.11 CATEGORY 11 - SAFETY, PERMISSIONS, GOVERNANCE
- [SRC high] Runtime modes and approvals: full-access, auto-accept-edits, approval-required (section 3.4); approval decisions accepted or rejected in chat (section 3.1).
- [SRC high] Agent browser gate: setting enableAgentBrowserAccess controls whether the MCP preview toolkit is exposed to agents.
- [SRC high] Confirmation settings: confirmQuit, confirmThreadArchive, confirmThreadDelete.
- [SRC high] Authentication stack: one-time pairing tokens (CLI pairing create, TTL); scoped bearer sessions (CLI session issue with --ttl and --subject; list and revoke); RPC authorization by scope (auth/RpcAuthorization.ts maps methods to scopes; for example, provider.uploadFeedback needs the operate scope); DPoP proof-of-possession (auth/dpop.ts with jose); OAuth config and a /oauth/token endpoint; EnvironmentAuth policy; PairingGrantStore; SessionStore.
- [SRC high] Secret storage: ServerSecretStore writes secret files under stateDir/secrets (directory mode 0700; files 0600; write to temp file then rename). Desktop: ElectronSafeStorage (Electron safeStorage, OS-backed); linuxSecretStorage.ts on Linux. Encryption at rest for the server secret files was not verified.
- [SRC high] Bootstrap secrets passed on a file descriptor (--bootstrap-fd) rather than argv.
- [SRC high] UI redaction: components/settings/RedactedSensitiveText.tsx.
- [SRC high] Path containment: workflow-script reads limited to the Claude projects folder by realpath (orchestration/workflowScriptQuery.ts); attachmentPaths.ts; pathExpansion.ts; atomicWrite.ts.
- [SRC high] Electron webview sandbox: preview webviews created with sandbox true (desktop preview/Manager.ts around line 2825); window webPreferences sandbox value is logged (ElectronWindow.ts around line 186).
- [SRC high] Telemetry gating by env (T3CODE_TELEMETRY_ENABLED); desktop telemetry on file descriptors (config.ts: desktopTelemetryFd, desktopTelemetryControlFd).
- [SRC high] Remote playbook trust boundary (DOC): the triage playbook text says agents fetch .github/triage/PLAYBOOK.md from the main branch and fall back to the bundled copy. Remote text from a branch can steer a coding agent. Not fetched during this audit.
- [SRC high] NOT OBSERVED: prompt-injection defenses (no matching identifiers); enterprise admin console, SSO, SAML, OIDC, audit logs, data-retention or zero-data-retention controls (sso and oidc hits: one generic). Accounts use Clerk on desktop and in the cloud (desktop.clerk.configure IPC).
- [SRC med] provider.uploadFeedback sends a Codex feedback upload through the Codex app-server (feedback/upload method, SM-server provider/Layers/CodexAdapter.ts around lines 1912-1919). The destination is on the Codex side (INF med).

3.12 CATEGORY 12 - UI/UX AND PRODUCT DESIGN
- [SRC high] Three-zone layout: left sidebar (projects, threads, grouping, snooze, pins, archived); center chat timeline with composer; right panel with tabs (diff, preview, PR, agents, files, terminal drawer), resizable, and a sheet on narrow widths. Pointer: SM-client components/AppSidebarLayout.tsx, RightPanelTabs.tsx, RightPanelSheet.tsx, ChatView.tsx, preview/RightPanelResizeHandle.tsx.
- [SRC high] Command palette: components/CommandPalette.tsx, CommandPaletteContent.tsx, CommandPaletteResults.tsx, CommandPalette.logic.ts; bus in commandPaletteBus.ts.
- [SRC high] Keyboard shortcuts with a customization file and page. Command ids seen: chat.new, chat.newLocal, composer.stash, diff.toggle, editor.openFavorite, preview.focusUrl, preview.refresh, preview.resetZoom, preview.toggle, preview.zoomIn, preview.zoomOut, sidebar.toggle, terminal.new, terminal.close, terminal.split, terminal.splitVertical, terminal.toggle, thread.next, thread.previous, thread.settle.
- [SRC high] Onboarding and pairing: connection/onboarding.ts; components/cloud/ConnectOnboardingDialog.tsx; components/auth/PairingRouteSurface.tsx (/pair); routes/connect.tsx and connect_.callback.tsx.
- [SRC high] Notifications and toasts: provider update launch and primary notifications; server update action; desktop update toast; keybinding update toast; slow-RPC toast coordinator. Dismissal states are stored in localStorage (for example t3code:version-mismatch-dismissals).
- [SRC high] Themes and appearance: see 3.5 and 3.8; contrast control, font pickers, glass opacity, theme preview wireframes.
- [SRC high] Accessibility: aria-* attributes in about 149 SPA files; Base UI primitives. Reduced-motion handling was not verified.
- [SRC high] i18n: no translation framework found (no i18n identifiers). Locale is used for timestamp formats and the desktop reads the system locale (desktop:get-system-locale).
- [SRC high] Performance: virtualized lists (@legendapp/list, patched); timeline scroll anchoring (components/chat/timelineScrollAnchoring.ts); AnimatedHeight and auto-animate; diff worker pool; LRU cache (lib/lruCache.ts); lazy route and asset chunks.
- [SRC med] Responsive layout: sheet-based right panel (RightPanelSheet.tsx). The mobile app is not part of this build.
- [SRC high] Desktop menu (ElectronMenu and DesktopApplicationMenu): File, View (Zoom In, Zoom Out, Actual Size), Settings, Check for Updates, DevTools toggles, reload, Open File and Open Folder. Context menus through desktop:context-menu.
- [SRC high] Window and theme sync: desktop:set-theme; syncAllAppearance; fullscreen state channels; splash window; app branding channel (desktop:get-app-branding).

3.13 CATEGORY 13 - PLATFORM ARCHITECTURE NOTES
- [SRC high] Process model: Electron main; one or more backend server processes (local, SSH, WSL); the SPA is served by the server and loaded in a window; IPC bridge through preload with desktop:* channel constants (SM-desktop ipc/channels.ts).
- [SRC high] Server structure: Effect layers (for example, Layer.mergeAll of ServerSecretStore and the SQLite persistence layer); services as Context.Service tags; repositories and projections over SQLite; event store plus command receipts for idempotency.
- [SRC high] Wire: RPC over WebSocket (/ws) with Effect RPC serialization; HTTP for assets, attachments and observability; MCP over HTTP (/mcp); NDJSON logs; contracts in packages/contracts (Effect Schema).
- [SRC high] Subscriptions: orchestration.subscribeShell (sidebar stream) and orchestration.subscribeThread (timeline stream); live refresh hook (hooks/useLiveRefresh.ts). Client state with TanStack Query and Effect atoms (state/*; @effect/atom-react pinned in overrides).
- [SRC high] Native pieces: node-pty (PTY); @ff-labs/fff (file search, C ABI library libfff_c); @yuuang/ffi-rs (FFI); t3-resource-monitor.exe (AMD64 PE, no version resource; implementation language not verified); Ghostty VT WASM (terminal parser and renderer).
- [SRC med] Rust-portability notes: the orchestration decider and projector are pure functions over events (orchestration/decider.ts, projector.ts) and port cleanly; the Codex app-server and ACP clients are JSON-RPC over stdio (portable to any runtime); PTY, file search and FFI are native dependencies to replace; the Effect service graph maps onto explicit traits or actors; the SQLite projection tables map directly onto a Rust store.
- [SRC high] Update distribution: electron-updater with GitHub releases; installer named "T3 Code (Alpha)"; Windows app user model id override env T3CODE_DESKTOP_APP_USER_MODEL_ID.
- [SRC high] Persistence roots: base dir from env T3CODE_HOME; stateDir is userdata (dev mode uses dev when a dev URL is set and no base dir is explicit) (SM-server config.ts around lines 108-134). File list in section 5.1.

==========================================================
4. EXTENSION POINTS (plugin, skill, MCP, API, CLI, webhook surfaces)
==========================================================
4.1 CLI front door: binary "t3" (SM-server cli/*, bin.ts) [DOC+SRC high]
- start: start the server. serve: run the server in the foreground. Flags seen: --mode, --port, --host (for example 127.0.0.1, 0.0.0.0, or a Tailnet IP), --dev-url, --no-browser, --bootstrap-fd, --base-dir, --auto-bootstrap-project-from-cwd, --log-websocket-events, --tailscale-serve, --tailscale-serve-port.
- service: install, update, uninstall, status (Linux systemd and macOS launchd only; see 3.6).
- project: add (workspace root, optional title), remove (optional delete of threads), rename.
- pair: issue a pairing link for a client; flags include label and Tailscale options.
- auth: pairing create, list, revoke (one-time client pairing tokens); session issue (scoped bearer token; flags ttl, subject, label, json, token-only, base-url), session list, session revoke.
- connect: login, link, status, publish (stop or start sending agent activity to mobile clients), unlink, logout. Reports "unavailable in builds without public configuration" when the build lacks config.
- triage: hand a misbehaving install to a local coding agent (claude or codex; flags for agent and model); writes context.md first.
- config: server flag documentation (CLI config module).

4.2 Remote API surfaces [SRC high]
- WebSocket RPC at /ws. Contract: packages/contracts/src/rpc.ts (string method names). Families: orchestration.* (dispatchCommand, subscribeShell, subscribeThread, searchThreads, getTurnDiff, getFullThreadDiff, getWorkflowScript, getArchivedShellSnapshot); projects.* and project.*; filesystem.browse; git.*; vcs.*; terminal.*; provider.* (uploadFeedback); server.* (config, settings, keybindings, updates, diagnostics, usage, process signalling, provider refresh, source-control discovery); preview.* and previewAutomation.*; review.*; sourceControl.*; pullRequests.* (list, detail, activity, comments, reviews, reviewers, reactions, thread resolution, runAction, diff file contents); cloud.*; attachments.*; shell.*; assets.createUrl; editor.openFavorite; auth.status.
- Authorization: auth/RpcAuthorization.ts maps each RPC method to a scope.
- HTTP: /pair (pairing page; token in URL fragment); /oauth/token (OAuth token endpoint); /api/attachments/upload; /api/assets; /api/observability/v1/traces (OTLP ingest proxy); /.well-known/t3/environment (environment descriptor for discovery).
- MCP over HTTP at /mcp (the preview toolkit; see 3.5).
- Desktop IPC (not for third parties): desktop:* channel constants in SM-desktop ipc/channels.ts.

4.3 Provider-side extension transports the server speaks [SRC high]
- Codex app-server: JSON-RPC over stdio (packages/effect-codex-app-server; generated schema; methods such as thread/realtime/started and model/rerouted; feedback/upload).
- Agent Client Protocol (ACP): packages/effect-acp over stdio (generated schema). Used for Cursor (command "agent stdio") and Grok, with extensions in provider/acp/*.
- Claude Agent SDK: query options (model, effort, permission mode, resume, session id, setting sources, additional directories, partial messages, path to executable, settings JSON, extra args JSON). Hook and plugin messages are mapped (see 3.4).
- OpenCode: launches "opencode serve" and uses @opencode-ai/sdk; SSE-style events are logged as opencode.sdk.event.

4.4 Local files a user can edit [SRC high]
- t3.json at the repository root (project config): scripts, runOnWorktreeCreate, previewUrl, autoOpenPreview, defaultThreadEnvMode, icon or iconPath.
- settings.json and keybindings.json in stateDir (see 5.1).
- Provider instance settings: binaryPath, launchArgs, homePath, shadowHomePath, environmentVariable, customModels (settings UI and settings.json).

4.5 Content extension points [SRC high]
- Themes: VS Code theme import (vscodeThemeImport.ts; desktop picker); Open VSX search and install (openVsxThemes.ts; VSIX read with jszip); custom theme editor (ThemeEditorPanel).
- Skills: provider skills are discovered (ClaudeSkills.ts) and shown in the slash menu (showSkillsInSlashMenu).
- Workflow scripts: read-only views of Claude workflow scripts (workflowScriptQuery.ts).

4.6 Not observed [SRC high]
- A T3 plugin manifest format, plugin marketplace, plugin SDK for end users, webhooks (0 identifiers), user-defined tool registration beyond MCP.
- The packages effect-acp and effect-codex-app-server are internal packages; they could serve as client libraries (INF med).

==========================================================
5. DATA AND PROTOCOL HINTS
==========================================================
5.1 Storage layout (SM-server config.ts; SRC high unless marked)
- Base dir from env T3CODE_HOME (default not verified). stateDir = <base>/userdata, or <base>/dev when a dev URL is set and no base dir is explicit.
- Under stateDir: state.sqlite (event store, projections, command receipts; 43 migration files); attachments/ (uploaded images); logs/ (NDJSON provider logs and server logs); keybindings.json; settings.json; anonymous-id; environment-id; server-runtime.json (runtime state for the running server); secrets/ (directory mode 0700; files 0600).
- Desktop: connection catalog (saved environments; desktop:get and set-connection-catalog), client settings (desktop:get and set-client-settings), app settings; electron-store and conf are present as libraries (exact files not read).
- Browser (SPA) localStorage keys with prefix t3code: renderer-state, sidebar-v (versioned), prompt-stash, resume-compaction-dismissed, version-mismatch-dismissals.
- Git: checkpoints stored as hidden refs in the workspace repository (CheckpointStore; does not coordinate provider rollback).
- Provider transcripts read by T3 (read-only, not read here): Claude project transcripts under a .claude/projects path; a sessions path for Codex (INF med). Workflow scripts live under the Claude projects folder.

5.2 Wire formats and protocols [SRC high unless marked]
- RPC: Effect RPC over WebSocket (RpcSerialization). Method names are string constants in packages/contracts/src/rpc.ts.
- Desktop IPC: channel names "desktop:*" (SM-desktop ipc/channels.ts; about 90 constants).
- Codex: JSON-RPC style app-server protocol on stdio.
- ACP: stdio, generated schema.
- MCP: HTTP at /mcp.
- Logs: NDJSON (EventNdjsonLogger); JSON Lines appear in provider transcript handling.
- Telemetry: PostHog (HTTP batches); OTLP traces and metrics (URLs from env).
- Schemas: Effect Schema for contracts; ajv and json-schema-typed in the desktop dependencies (use not traced) [SRC med].
- msgpackr is a dependency on desktop and server (use not traced) [SRC med].

5.3 Auth and identity [SRC high unless marked]
- Pairing: one-time pairing tokens; the link format is /pair#token=... (CLI auth pairing create prints the link).
- Sessions: scoped bearer access tokens with a TTL; listed and revoked from the CLI; RPC calls checked against scopes.
- DPoP: auth/dpop.ts (proof-of-possession binding); jose 6.2.2.
- OAuth: config types (OAuthConfig) and /oauth/token.
- Accounts: Clerk (clerk-js, @clerk/electron, passkeys package) for desktop and cloud sign-in.
- Tailscale: Tailscale Serve and status are used; identity mode not verified.
- Provider logins: each provider CLI keeps its own login; T3 checks gh and glab auth status (gitHubAuthStatus.ts, gitLabAuthStatus.ts).
- Bitbucket: email plus API token, or an access token (env names only; see 5.4).

5.4 Environment variable NAMES (values never read) [SRC high]
- Server and desktop: T3CODE_PORT, T3CODE_MODE, T3CODE_HOST, T3CODE_NO_BROWSER, T3CODE_HOME, T3CODE_DESKTOP_WS_URL, T3CODE_DESKTOP_LAN_ACCESS, T3CODE_DESKTOP_LAN_HOST, T3CODE_DESKTOP_HTTPS_ENDPOINTS, T3CODE_TAILSCALE_SERVE, T3CODE_TAILSCALE_SERVE_PORT, T3CODE_DESKTOP_APP_USER_MODEL_ID, T3CODE_DEV_REMOTE_T3_SERVER_ENTRY_PATH (dev-only), T3CODE_COMMIT_HASH, T3CODE_DISABLE_AUTO_UPDATE, T3CODE_DESKTOP_MOCK_UPDATES (test hook), T3CODE_DESKTOP_MOCK_UPDATE_SERVER_PORT (test hook), T3CODE_OTLP_TRACES_URL, T3CODE_OTLP_EXPORT_INTERVAL_MS.
- Telemetry: T3CODE_TELEMETRY_ENABLED, T3CODE_TELEMETRY_FLUSH_BATCH_SIZE, T3CODE_TELEMETRY_MAX_BUFFERED_EVENTS, T3CODE_POSTHOG_KEY (secret-like; name only), T3CODE_POSTHOG_HOST, WSL_DISTRO_NAME (read).
- Source control: T3CODE_BITBUCKET_EMAIL, T3CODE_BITBUCKET_API_TOKEN, T3CODE_BITBUCKET_ACCESS_TOKEN (secret-like; names only).
- Settings key serverPassword is secret-like (name only).

5.5 Settings keys (packages/contracts/src/settings.ts; grouped; full list in source) [SRC high]
- Appearance: appearanceContrast, fontFamilyCode, fontFamilyComposer, fontFamilySans, fontFamilyTerminal, fontSizeCode, fontSizeInterface, fontSizePrompt, fontSizeTerminal, fontSmoothing, glassOpacity, timestampFormat, wordWrap, legacySidebarEnabled, sidebarProjectGroupingMode, sidebarProjectGroupingOverrides, sidebarProjectSortOrder, sidebarThreadSortOrder, sidebarThreadPreviewCount, sidebarAutoSettleAfterDays, sidebarAutoSettleOnMerge, environmentIdentificationMode, confirmQuit, confirmThreadArchive, confirmThreadDelete, favorites.
- Browser preview: browserAutoShowFloatingPreview, browserDefaultAppearance, browserDefaultViewport, browserDefaultZoomFactor, enableAgentBrowserAccess.
- Threads and git: defaultThreadEnvMode, newWorktreesStartFromOrigin, addProjectBaseDirectory, diffIgnoreWhitespace, automaticGitFetchInterval, sourceControlWritingStyle, sourceControlWriterModelSelection, followChangeRequestTemplates, textGenerationModelSelection.
- Agents: planModeEnabled, autoCompactWindow, showSkillsInSlashMenu, enableLegacyTokenStreaming, model, customModels.
- Providers: providers, providerInstances, providerInstanceId, providerModelPreferences, binaryPath, launchArgs, homePath, shadowHomePath, environmentVariable, providerHealthRefreshInterval, enableProviderUpdateChecks, dismissedProviderUpdateNotificationKeys.
- Background: backgroundActivity, backgroundActivityProfile, hostPowerMonitorActiveInterval, hostPowerMonitorIdleInterval, idleClientTtl, pauseWhenClientLowPower, pauseWhenHostLocked, pauseWhenHostLowPower, pauseWhenOnBattery.
- Server and connection: serverUrl, apiEndpoint, serverPassword (secret-like), instanceId, observability, schemaVersion, settingsPath.

5.6 Keybinding command ids (contracts/keybindings.ts and SPA) [SRC high]
- chat.new, chat.newLocal, composer.stash, diff.toggle, editor.openFavorite, preview.focusUrl, preview.refresh, preview.resetZoom, preview.toggle, preview.zoomIn, preview.zoomOut, sidebar.toggle, terminal.close, terminal.new, terminal.split, terminal.splitVertical, terminal.toggle, thread.next, thread.previous, thread.settle. The default key map was not found in the searched files.

5.7 Orchestration commands and events (packages/contracts/src/orchestration.ts) [SRC high]
- Thread commands and events: thread.create, thread.archive, thread.unarchive, thread.delete, thread.pin, thread.unpin, thread.pin.reorder, thread.snooze, thread.unsnooze, thread.settle, thread.unsettle, thread.meta.update, thread.title.regeneration.complete, thread.session.set, thread.session.stop, thread.runtime-mode.set, thread.interaction-mode.set, thread.turn.start, thread.turn.interrupt, thread.approval.respond, thread.user-input.respond, thread.checkpoint.revert, thread.activity.append, thread.proposed-plan.upsert, thread.token-usage.updated, thread.message.assistant.delta and .complete, thread.realtime.* (started, item-added, audio.delta, closed, error).
- Turn events: turn.started, turn.completed, turn.aborted, turn.diff.updated, turn.plan.updated, turn.proposed.delta, turn.proposed.completed.
- Other: model.rerouted; project.create, project.delete, project.meta.update.

5.8 Enum literals (contracts; SRC high)
- Runtime modes: full-access, auto-accept-edits, approval-required.
- Codex sandbox values: read-only, workspace-write, danger-full-access.
- Interaction modes: default, plan.
- Environment modes: local, worktree (and local-only appears in the server exposure enum).
- Server exposure: local-only, network-accessible.
- Approval decisions: accepted, rejected. Approval and user-input status: pending, resolved.
- Thread settle status: settled, active. Background status: working, monitoring.
- Sidebar sort: updated_at, created_at, manual. Timestamp format: locale, 12-hour, 24-hour. Streaming: buffered, streaming.

5.9 Dependencies (full name@version lists are in files; no need to re-read the bundles)
- SCR/tmp/t3code/deps_desktop_flat.txt (116 packages, desktop node_modules, from the extracted copy)
- SCR/tmp/t3code/deps_server_flat.txt (16 packages, server node_modules, from the extracted server copy)
- SCR/tmp/t3code/deps_client_pnpm.txt (147 packages, from the SPA bundle map paths; two tanstack versions truncated: react-router 1.17.x, react-pacer 0.19.x)
- SCR/tmp/t3code/deps_server_pnpm.txt (21 packages, from the server bundle map paths)
- Notable server packages: @opencode-ai/sdk 1.15.13; jose 6.2.2; @noble/curves 1.9.1; @noble/hashes 1.8.0; msgpackr 2.0.4; ws 8.21.0; find-my-way-ts 0.1.6; multipasta 0.2.8; cross-spawn 7.0.6; which 2.0.2; yaml 2.9.0; toml 4.3.0.
- Notable desktop packages: electron-updater 6.8.9; playwright-core 1.60.0; react-grab 0.1.50; bippy 0.6.1; agent-install 0.0.6 (not referenced by app-owned code; INF unused or build tooling); @stripe/stripe-js 5.6.0 (no Stripe usage found in app-owned code; INF unused); fast-check 4.9.0 (test library shipped; INF low); @zxcvbn-ts/core 3.0.4 (password strength; INF med, likely via Clerk).

==========================================================
6. BLOCKERS AND LIMITS
==========================================================
- REA: two desktop inspections and one server trace returned results larger than the MCP transport budget. They stay retained (IDs in 0.3). Narrower paged views or a dedicated export would be needed to read them.
- REA: all semantic families are partial; the server analysis recovered 0 modules. REA gave no route, channel or module graph, so every feature fact here is SRC-based.
- REA: the desktop trace with an exact IPC string seed found no match, so REA has no desktop IPC evidence.
- The app was NOT launched. Runtime behaviour, UI states and defaults are unverified (for example telemetry default, keybinding defaults, auto-settle defaults, first-run flows).
- provider/model-manifest.json is 0 bytes in the recovered source map; model identifiers come from string constants in the server bundle, which are code constants and not a live model list.
- The Claude Agent SDK is referenced in the server bundle map but its code and version were not extracted.
- Windows branches were not read for PortScanner, keep-awake, or service behaviour (the service command is documented as unavailable on Windows).
- The SPA repo path (apps/web) is inferred from relative source-map paths; file names are exact.
- Two TanStack versions are truncated in the pnpm path parse (react-router 1.17.x, react-pacer 0.19.x). The Ghostty VT WASM version is not recorded.
- The resource-monitor implementation language is not verified (PE has no version resource; file is AMD64). The libfff_c language is not verified.
- No git history: the install is not a repository. Commit f925d6394218 comes from package.json only.
- Not read, by rule: C:/Users/ImaFernandesDaCosta/AppData/Roaming/t3code, AppData/Local/t3code-updater, any user data, cookies, tokens, and real .env secrets. No secret-bearing file was opened (env and settings NAMES only; values never read). The REA result file under the Claude projects folder was read only for this session's own tool output.
- The triage playbook was not fetched (no network use in this audit).
- Orientation: the CoresSystem CLAUDE.md and apps/CODEBASE_INFORMATION_SYSTEM.md were read (head only). No mapping to Verevon was attempted (out of scope for this scout).
- Keyword scans are presence-only; generic words produced false positives (hooks, review, test, fallback, stripe, injection). Negative findings are marked NOT OBSERVED and are only as strong as those searches.
- Sibling artifacts in findings/t3 (other products) were not read.
