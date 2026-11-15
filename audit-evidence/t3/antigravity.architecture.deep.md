# Google Antigravity 2.11.0: architecture deep dive (facet: architecture)

Audit 3 input for Verevon (Model Plane plus chat/agent pages). Read-only analysis of the installed program files and the pre-extracted copies; nothing was launched, no project code was run, no user-data store was read (`~/.gemini/*`, `AppData/Roaming/Antigravity` skipped; no secret was printed or recorded).

Companion file: `findings/t3/antigravity.architecture.features.json` (104 records, ids `antigravity-architecture-001..104`). Categories used: 13 Platform architecture (33), 4 Agent runtime (20), 11 Safety/permissions/governance (18), 6 Computer and environment control (14), plus a few records in 5, 9, 10, 12, 1, 2, 3, 7, 8.

## 0. Provenance legend and evidence conventions

- REA = observed through REA analysis of a shipped artifact. SRC = read directly in shipped files (JavaScript source, package files, binary strings, Go symbol names, embedded prompts and docs). DOC = public documentation (antigravity.google/docs, fetched) or the product's own embedded docs. INF = my inference, basis stated.
- Evidence pointers: `extract/antigravity/dist/<file>:<line>` is the pre-extracted shell source. `language_server.exe@<offset>` is a byte offset into the 153 MB Go backend (strings extracted to `tmp/agarch/ls_strings.txt`, embedded doc range dumped to `tmp/agarch/ls_docs_48M.txt`). `LS-embedded-UI main.js@<offset>` is a character offset into the UI bundle that I extracted from a zip inside the backend (`tmp/agarch/ui/main.js`). Go symbol names come from the binary's pclntab (`tmp/agarch/ls_go_pkgs.txt`); they prove a package or function exists, not how it behaves.

## 1. Corrections to the brief and to the scout's inventory

1. This PC has the agent hub (Antigravity 2.x, "agent manager" app), not the VS Code-based editor. The editor is a separate product ("Antigravity IDE") offered by a first-run wizard and is not installed (Programs lists `antigravity`, not `Antigravity IDE`). The "fork structure" part of the facet could therefore not be examined (no `product.json`, extension host or workbench). What is visible is the editor's bridge into the shared backend (`ExtensionServerService`) and the UI's IDE-webview mode. [SRC; records 001, 062, 101]
2. The scout wrote that the UI framework was unverified and that the UI bundle was not extracted. Resolved: the UI is a 300-entry zip appended inside `language_server.exe` (EOCD at file offset 130,646,139, base 127,470,842); `main.js` is 8.9 MB of Google Closure Compiler advanced-mode output over React (`createRoot`, `createElement`), with xterm, Mermaid, KaTeX, react-markdown/remark and Prism. Title "Jetski Web". [SRC/REA; record 008]
3. REA's `sender_validation_observations: 33` is not backed by the source: a direct grep of the non-test shell files finds no `senderFrame`, origin, or `webContents` checks in any IPC handler, and no `will-navigate`/permission handlers. Treat IPC as unvalidated. [SRC; records 013, 081]
4. The scout reported no prompt-injection defence phrases. The backend does carry prompt-layer defences (instruction/system-prompt protection with a decoy answer, a config-file security reviewer with an injection guard, a denied-action equivalence analyzer) and a data-flow vetting layer. [SRC; records 070-074]
5. SQLite use (scout: low) is upgraded to medium-high: GORM sqlite driver symbols and `trajectory_meta` SQL strings are present. [SRC; record 014]
6. Public docs state that on Windows the older permission system is still used until an update; the new grant engine and sandbox may not be active on this machine even though `exebox` (a Windows sandbox) is compiled in. [DOC; records 059, 066]

## 2. What the product is

An Electron desktop hub whose entire product logic lives in one Go binary (internally "Jetski", agent trajectories called "Cascade", lineage visible from the Codeium/Windsurf language server: `exa.*` protos, `x-codeium-csrf-token`, `CODEIUM_LANGUAGE_SERVER_BIN`). The same binary is the backend for four front ends: the hub UI, the `agy` CLI/`agentapi`, the Python SDK runtime (shipped inside pip wheels), and IDE extensions. Public docs list editor extensions for VS Code, Visual Studio, JetBrains, Zed and Xcode. [SRC/DOC; records 004, 101]

Install footprint: about 523 MiB: `Antigravity.exe` 222.8 MB (Electron), `language_server.exe` 153.1 MB, `webm_encoder.exe` 13.6 MB, `app.asar` 4.5 MB, unpacked `chrome-devtools-mcp` 1.1.0 (third_party bundle 16.4 MB). All five Windows executables are NotSigned by Authenticode. [SRC; records 103, 082]

## 3. System architecture

```
 user clients                      desktop host (Electron main)                backend (Go, one binary)
 ------------                      ---------------------------                 ------------------------
 hub window (renderer)  <--https-->  [thin shell: window, tray, updater]  --spawn-->  language_server.exe
   loads https://127.0.0.1:port       host bridge (Connect, loopback,        |  Connect RPC over HTTPS (exa.* protos)
   preload: 10 contextBridge APIs      bearer token)  <---callbacks----------|  /connect-websocket (multiplexed)
 web / PWA / mobile (remote control) ------ WebRTC data channel (pion) -------|  embedded UI zip, CSRF-protected
 IDE webview (extensionView) --- WebSocket + editor RPC router ---------------|  agent loop (cortex), tools, MCP client
 agy CLI / agentapi / SDK / sidecars --- Connect + CSRF ----------------------|  sandbox (exebox), policy guardian
                                                                              |  gorm+sqlite, git snapshots
 child processes: chrome-devtools-mcp (on Electron-as-Node), webm_encoder, user MCP servers, sidecars, hooks, shells
```

### 3.1 Process model (record 002, 003)
- `main.js` creates a per-launch random CSRF UUID, starts the loopback host-bridge server, then spawns the backend with `--standalone --subclient_type hub --https_server_port 0 --csrf_token ... --api_server_url https://generativelanguage.googleapis.com --cloud_code_endpoint https://daily-cloudcode-pa.googleapis.com --enable_sidecars --host_bridge_url/--host_bridge_token`. It parses the port from a log banner (`listening on ... port at N for HTTP(S)`), then `loadURL(https://127.0.0.1:<port>/)`. [SRC main.js:195-249, languageServer.js:186-239]
- Crash supervision: stderr ring buffer scanned for Go crash phrases, up to 3 restarts per 60 s with a 2 s cooldown, windows reload on the new port, then a silent "Giving up". [SRC languageServer.js:339-386]
- The shell is deliberately thin: 28 IPC handlers, 10 `contextBridge` APIs, 0 utility processes, 0 native addons (REA: 561 modules, 61 IPC operations). [REA ev_07f3b307...]

### 3.2 Protocols (records 005, 006, 007, 012, 019, 020)
- Connect protocol v1 over HTTP: `application/proto` or JSON for unary, `application/connect+proto|json` for streams, GET for idempotent unary. A second transport multiplexes every RPC over one WebSocket (`/connect-websocket`) with JSON frames `{streamId, type: start|cancel|data|end, procedure, stream, headers, payload, statusCode, error}`; the CSRF token travels in the frame headers. Used for IDE webviews or `?useWebSocket=true`. [SRC UI@3048200-3060500]
- State is pushed, not polled: `StreamAgentStateUpdates` carries trajectory, step, queued-message, background-task, artifact-snapshot and file-diff updates; plus a generic reactive-component stream and unified state-sync topics. The UI keeps a 50-entry RPC ring buffer (`globalThis.__rpcdb`). [SRC UI@1077300, 1546888]
- Auth: CSRF token (header `x-codeium-csrf-token`, also `?csrf=` on `/static/artifacts/...`), on auth error the UI refetches `index.html` and regex-extracts a fresh token. Host bridge uses a separate 32-byte bearer token so a compromised page cannot trigger updates. [SRC]
- Remote control: pion WebRTC data channels, DTLS fingerprint canonicalisation, ECDSA P-256 channel-binding signatures, mesh daemon and transport multiplexer; paired by signing into a web dashboard with the same Google account; CLI instances use reverse tunnels or a background daemon (systemd user service, LaunchAgent, Scheduled Task). [SRC symbols; DOC docs/remote-control]
- SSH remoting: init scripts start a remote backend on port 0, read the banner, keep the SSH/SOCKS tunnel alive. [SRC; record 020]

### 3.3 One UI for many hosts (records 008-010, 027)
The UI receives host capabilities as injected providers (`featureManager`/`featureManagerV2`, `appVariant` internal vs external, update service, clipboard/workbench services, aux-pane registry). Client surfaces are named strings: `electron`, `web.desktop|mobile.(remote-control).(pwa)`, `extension.cider|vs-code|visual-studio|eclipse`. `extensionView=true` switches to in-memory routing and an editor RPC router (`getEditorState`, `onAntigravityReady`). Flags `cloudAgentsEnabled`, `useMachineLSEnabled`, `sidecarMode` are false in this build, but the UI already has local/machine backend client managers and an instance selector. [SRC UI@8914500-8925616]

### 3.4 Storage (record 014)
- Shell: `userData/app_storage.json` (string KV, read-modify-write, no lock, broadcast to windows); three shell settings (`runInBackground`, `keepComputerAwake`, `autoCheckForUpdates`).
- Backend: `~/.gemini/config/config.json` (proto-JSON `UserConfig`: user settings, sidecars), data dir `~/.gemini/antigravity` (CLI: `antigravity-cli`, IDE: `antigravity-ide`), gorm+sqlite (`trajectory_meta`, generator/executor metadata, parent refs), `ls_<id>.json`, `.pb` snapshots, `transcript.jsonl`/`transcript_full.jsonl`, `mcp_oauth_tokens.json`, git refs `refs/snapshots/` and bundles for checkpoints. Credentials for Google OAuth go to the OS keyring (zalando/go-keyring + wincred) with WSL/SSH/D-Bus/container detectors. [SRC]

### 3.5 Update and distribution (records 015, 016, 082)
electron-updater with a generic Cloud Run manifest, channel `latest-<arch>(-win)`, first check at 10 s then hourly, auto-download, install on quit. The backend can query and trigger updates through the host bridge. Headless Linux replaces the AppImage via a detached shell script. Gaps: `app-update.yml` has no `publisherName`, so `NsisUpdater.verifySignature` returns null (no Authenticode check); all Windows binaries are unsigned; the IDE installer is downloaded to a fixed temp name and executed with `/VERYSILENT` with no hash. [SRC extract/.../NsisUpdater.js:83-99, ideInstall/service.js]

### 3.6 Telemetry (records 024, 083)
UI and backend events go through a backend-hosted Clearcut proxy (`clearcut_proxy`), with playlog protos and a CEL-based telemetry filter; the UI logs under `logSource 2999`; `disableCodeSnippetTelemetry` is a user setting. The bundled DevTools MCP would collect tool name, bucketed latency and length-bucketed parameters (its `sanitizeParams` never sends values), but the backend launches it with `--no-usage-statistics --no-performance-crux`. The preview pane reports URL type and host. No ZDR concept was found. [SRC]

## 4. Agent runtime architecture

- Layering: a product-agnostic framework (`gemini_coder/framework`: core, agent, executor, registry, stepmanager, task, tracer, handlers/checkpoint, handlers/applyedit) under a product layer (`jetski/cortex`: tools, handlers, executors, customizations, hooks, permissions, policyguardian, command, snapshot_recorder, battlemode, sidecars, trajectory). [SRC symbols; record 031]
- Data model: a trajectory is a sequence of typed steps (122 step types; statuses PENDING, RUNNING, WAITING for approval, DONE, ERROR, INVALID, CANCELED), persisted in sqlite, restorable (rehydration), forkable. [records 032]
- Hook pipeline: `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation`, `Stop` in `hooks.json`; command hooks only, synchronous, JSON stdin/stdout in camelCase; PreToolUse returns `allow|deny|ask` plus `permissionOverrides`; PostInvocation can inject steps or force continue; Stop can block stopping. Same shape as Claude Code hooks. [DOC; record 033]
- Composition: `CustomizationMixinWrapper` merges skills, rules, plugins, MCP servers and custom agents into prompt text, tool options, step renderers, handler decorators and hooks. Harness variants (`rsi_lean_di` safe/risky, `minisweagent`, `minimalagent`, `google_no_search_tools`, `prodvariants`) are selectable by experiment key. [SRC symbols; records 034, 035]
- Multi-agent: built-in "owl" decides Solo vs Delegation to hidden `DeepCoder`/`DeepInvestigator`; "Teamwork" prompts define pipeline (conductor, sentinel, stages) and swarm (orchestrator, workers, synthesizer, critic orchestrator) coordinated by messages plus markdown blackboards in the workspace (`.pipeline_progress_<task>.md`, `.swarm_blackboard_<task>.md`), reactive wakeup instead of polling, worker workspaces `branch` (isolated clone) or `inherit` (new files only). Battle Mode runs best-of-N arms in cloned workspaces with a pick/apply step. [SRC prompts; records 036-038]
- Control surfaces: queued messages (Queue vs Send Immediately), background tasks with `fullyIdle`, checkpoint/revert (git snapshot refs and bundles), sidecars (directory-defined supervised scripts with a built-in cron `schedule`, hot-reload, logs/events, `agentapi` calls), `agentapi` RPC CLI, Python SDK (read-only by default). [SRC/DOC; records 039-043, 049]
- Quality-of-loop details worth noting: an edit-repair model call for failed search/replace chunks (`046`), step renderers as Go templates separating model text from UI (`048`), typed credit/quota/capacity errors (`047`), knowledge items, transcripts and compaction notices (`044`), plan/walkthrough artifacts with approval (`045`).

## 5. Computer and environment control

- Browser: pure CDP, no Chrome extension channel (no `nativeMessaging`/`chrome.debugger` strings). The agent drives the user's installed Chrome in a separate profile (`~/.gemini/antigravity-browser-profile`) on CDP port 9222 (all configurable), through a Browser Subagent. Tooling is `chrome-devtools-mcp` 1.1.0 launched from the shell-provided entry point (`CHROME_DEVTOOLS_MCP_JS`) with a PATH shim `agy-node` that runs `Antigravity.exe` with `ELECTRON_RUN_AS_NODE=1`; Go also contains CDP bindings, a browser launch manager, state diffing and Playwright-go symbols. [SRC; records 052-055]
- DevTools MCP internals worth copying: one global mutex serialising tools, optional page-id routing for concurrent agents, per-category enablement, `--slim` 3-tool mode, vision tools off by default, navigation allow-list parameter, header redaction, and `validatePath` that canonicalises paths and confines them to MCP roots (+ tmpdir). It also has WebMCP (page-exposed tools) and extension install/trigger tools, off by default. [SRC; records 054, 056]
- Recording: the backend embeds `webm_encoder.exe`, a Go helper that drives a hidden tab of the hub's own Chromium (the shell forces `--remote-debugging-port=0` and records `DevToolsActivePort` for it): a Go-template JS creates a canvas, `captureStream(0)`, `MediaRecorder(video/webm;codecs=vp9)`, draws each screenshot and requests a frame, returning base64 chunks over a CDP binding. No ffmpeg needed. [SRC; record 021]
- Terminal: backend-owned shell with real shell-grammar parsing (mvdan/sh, ANTLR), PowerShell-specific matching, background commands, command setup script, output truncation. [SRC; record 058]
- Sandbox: `exebox` (Windows AppContainer + job object + separate desktop + per-path capability SIDs + loopback SID handling + firewall API + admin broker pipe + local CA for a TLS-intercepting egress proxy) plus `sandbox/{sbox,proxy,urpc,localcert}`. Public docs: the Default preset runs commands in the sandbox (workspace and temp only, no network), granted paths and domains extend the allow lists, escapes require approval. No seatbelt/landlock/bubblewrap identifiers: the non-Windows implementation is not visible here. [SRC/DOC; record 059]
- Workspaces: projects, scratch projects, tracked workspaces, git worktrees/clones for parallel agents (settings `maxClonedWorkspaces`, `maxClonesPerWorkspace`, `disableEagerCloning`, `bestOfNFullVcsClone`), VCS UI strategies for git, jj, fig, gob. [SRC; record 060]
- There is no desktop-wide computer use; a computer-use model enum exists (name only). [INF low; record 065]

## 6. Safety, permissions and governance

Layers, outermost first (all SRC unless noted):

1. Posture presets and per-project overrides: Default, Request Review, Turbo (enum also has Vetted and None); terminal policy Always Proceed / Request Review / Proceed in Sandbox / Auto; file access (allow/ask/deny) and internet access (allow/ask/deny) policies; browser tools toggle, JS execution policy (Disabled / Request Review / Always Proceed), allow/deny URL lists. [records 067, 075, 061]
2. Grant engine: `action(target)` rules (`read_url(*)`, `execute_url(*)`, `write_file(path)`, `unsandboxed(*)`), global/project/conversation scopes, Deny over Ask over Allow, write implies read. PreToolUse hooks can add overrides. [DOC + symbols; record 066]
3. Command lists: Unix prefix-token matching, PowerShell contiguous-subsequence matching, deny beats allow, `IsDangerousBinary`, allow-always suggestions, explainable `AutoRunDecision` (default/model/system/user). [record 068]
4. Strict Mode: a switch that disables the most autonomous options and locks settings (UI shows "managed by your organization" and "strict mode" lock reasons). [record 069]
5. Policy Guardian (`vet_data_flow`, `vet_content`, `vet_dependency`, `vet_shell_change`, `vet_unknown_behavior`) with taint tracking of downloaded paths (`env_state.untrusted_downloads`, `source_in_downloaded_path`), secret-type data labelling, fail-closed/fail-open degradation, malicious-package, Safe Browsing and antivirus checks. Probably enterprise or Google-internal gated; unverified for external users. [records 070, 071]
6. Denied-action equivalence analyzer: prevents retrying a denied action by rephrasing. [record 072]
7. Config-change reviewer prompt: registry hijack, protocol downgrade, exfil endpoints, access-control weakening; tells the model to ignore instructions inside the file. [record 073]
8. Prompt-level confidentiality: "Instruction Protection" and "System Prompt Protection" with a decoy reply. Not a security boundary. [record 074]
9. Credential brokering: the permissioned-GitHub skill defines `gh.<action>({org,repo,pr|issue|contents|branch|path,ref})` grants; Go symbols (`buildGitHubNetboxConfig`, `configureTrustedCLIs`, `initCA`, `sandboxproxy`) indicate enforcement at a TLS-intercepting egress proxy so the agent never holds a token (INF for enforcement). [record 077]
10. Artifact review mode with an explicit injection warning (Always Proceed disabled in strict mode). [record 076]

Local attack surface and integrity (all SRC, severity INF): the shell appends `--remote-debugging-port=0` to its own Chromium unconditionally, so a same-user process can read `DevToolsActivePort` and attach to the renderer and its CSRF token; `setCertificateVerifyProc` accepts any certificate for `127.0.0.1`/`localhost` for the whole session; IPC handlers do not validate senders; `extensions:send-authorities` lets the renderer register arbitrary proxy targets for `plugin://`; CSRF and bridge tokens are passed on argv; artifact URLs carry `?csrf=`; Electron fuses are defaults (RunAsNode on, asar integrity off, cookie encryption off). Update/installer integrity gaps as in 3.5. [records 081, 082, 030, 013, 012, 018]

## 7. Best-in-class elements

1. One backend binary reused by hub, CLI, SDK, IDE extensions and remote machines (record 004). The cleanest reuse story in this audit set.
2. Typed trajectory of steps with an explicit WAITING-for-approval state; step renderers separate model text from UI (032, 048).
3. Permission model: uniform `action(target)` grammar, scoped grants, fixed precedence, presets with per-project overrides, Strict Mode (066-069).
4. Defence in depth that goes beyond lists: taint tracking of downloads, data-flow vetting, denial equivalence, config-change review, brokered credentials (070-073, 077).
5. One UI for six host surfaces via injected providers; multiplexed Connect-over-WebSocket transport (008-010, 006).
6. Customization system: skills/rules/hooks/MCP/sidecars/plugins in one layered root with inheritance and repo-committed registries (050, 033).
7. Remote supervision: WebRTC remote control and daemonised CLI (019, 100).
8. Self-documenting agent with offline references and live-docs fallback (022).
9. State-diff reporting for browser observations; roots-confined file paths in the browser tool server (055, 054).
10. Browser recording by reusing the host Chromium as a video encoder (021).

## 8. Weaknesses and limitations

- Security hygiene of the desktop host is below the backend's design quality (section 6 local attack surface; unsigned binaries; no updater signature check).
- Silent failure after 3 backend crashes; in-flight runs die with the backend.
- One-at-a-time browser tool execution (global mutex) limits parallel browsing; concurrency relies on experimental page-id routing.
- Large mixed-heritage proto surface (Codeium legacy, Google-internal steps such as BLAZE/CitC/Cog/Fig) compiled into one external binary (record 104).
- Permissions: the new engine and sandbox are documented as not yet on Windows; Windows users are on the older auto-execution system.
- Hooks are command-only and synchronous; no HTTP/prompt hooks yet.
- No general desktop computer use; browser-scoped only.
- No ZDR concept visible; telemetry flows to Google logging by default (first-party opt-out defaults unknown).
- The Closure-compiled UI is opaque (hard to audit); parts of safety logic may live there (strict-mode locks).

## 9. What Verevon should copy, adapt, or avoid

Mapped to the architecture rules in `CLAUDE.md` (no cross-plane DB access; Model proposes, Quarry executes; ZDR propagates; Space access never widens an owner-plane ACL).

Copy or adapt
- Model Plane as one runtime with three front doors (gateway service, CLI, embeddable SDK). Keep a product-agnostic core (loop, executor, tracer, step manager) separate from product tools and personas. (004, 031)
- Run record as a typed step trajectory with WAITING-for-approval; stream typed deltas, not polling. (032, 007)
- Adopt the `action(target)` permission grammar with scopes and Deny > Ask > Allow, presets, per-project/Space overrides and a Strict Mode that admins can enforce server-side. This lines up with Control Plane policy plus Quarry grants. (066-069)
- Implement a vetting layer over tool calls: taint-track downloaded artifacts, classify data flows (source, destination, intent, data type), remember denials with semantic equivalence, and screen writes to config/rc files. Feed it from Quarry and integration-corev2 effects. (070-073)
- Credential brokering at the egress proxy with resource-scoped typed grants for integration actions (permissioned-GitHub pattern) so agents never see tokens. (077)
- Hooks (tool gate, post-tool, stop guard) plus HTTP hooks; skills as the single procedure format; layered customization roots with inheritance and committed registries. (033, 050, 097)
- Multi-agent via blackboard documents plus messages, backed by Data Plane documents instead of workspace files; best-of-N arms with isolated sandbox volumes. (036-038)
- Browser observation as state diffs; roots-confined file paths; per-host page-access prompts; JS-execution policy. (055, 054, 075)
- Remote supervision surface (mobile/PWA) and customer-hosted runner daemons. (019, 100)
- Embedded product-docs skill for the agent; provider-injection UI architecture for SolidJS so chat/agent pages run in gateway, embedded and partner hosts; embed UI into the Rust gateway binary. (022, 009, 008)
- Debug bundle and RPC ring buffer for support. (025)

Avoid
- Secrets on argv, tokens in artifact URLs, unauthenticated debug ports, unvalidated IPC, unsigned artifacts, unsigned installer downloads (012, 081, 082, 016).
- Full-environment inheritance into agent processes (build an explicit env in sandboxes). (063)
- Mixing internal and external variants in one shipped binary. (104)
- Silent supervisor give-up. (003)

Rust portability notes (INF, not verified against crate maturity): Connect over HTTP and WebSocket frames can be served from axum with prost-generated types (verify Connect-protocol support or write a thin shim); the pion/WebRTC stack has a Rust counterpart (`webrtc` crate); shell parsing for policy can use a tree-sitter or dedicated bash parser; microVM isolation replaces AppContainer for Verevon's Linux sandboxes; SQLite+git-ref snapshots map to workspace-volume snapshots in sandboxes.

## 10. Gaps of Antigravity versus Verevon's stated goals

- Multi-model transparency in the style of DeepSeek-harness (token/log level) is not evident; cost summaries and verbose thinking are.
- Memory learning loop (Hermes-style) is present only as KI/brain/memory primitives; quality not observable.
- No general computer-use (Manus-style); browser only.
- No built-in multi-model comparison UI beyond Battle Mode arms.
- No ZDR/propagation design. No Space/ACL concept (projects and workspaces only).
- No Office-document/slide generation steps found (absence from string search only).

## 11. Open questions

1. Which permission/sandbox implementation is active on Windows in 2.11.0 (older auto-execution system per docs vs `exebox`)? Needs runtime observation, which this audit forbids.
2. Is the Policy Guardian active for external accounts, and under which preset (VETTED)? Strings include internal hosts and Archboard/datapol protos.
3. What does the LS-side `/connect-websocket` handler enforce (origin check, CSRF)? Not located by string.
4. How does the backend launch chrome-devtools-mcp exactly (argv, `agy-node`)? Only adjacent flags and env names are visible.
5. Is the hub's `--remote-debugging-port=0` also used for anything besides the encoder, and is it reachable beyond loopback? (Chromium default is loopback.)
6. Non-Windows sandbox implementation (macOS/Linux) not present in this build.
7. The "Local models" and "Personas/Policies/Structured output" SDK pages in the docs navigation were not fetched; SDK capabilities beyond the quickstart are unverified.
8. The VS Code-based IDE structure (product.json, extension host, bundled extensions, browser extension channel) requires installing/inspecting `Antigravity IDE`, which is not on this PC.

## 12. Tool and method log

REA tools actually used
- `analyze_javascript_application` on the extracted UI bundle directory (`tmp/agarch/ui`, format directory, detail summary, record-and-continue, heap 2048 MB, timeout 600 s): 6 relevant files, 4 JS files parsed, 1,579,792 AST nodes, 496 findings; semantic pass partial because the worker exhausted its heap on `main.js` (evidence ev_9747fee3... parent, ev_7a45aa00... summary). This is a Closure bundle, so no Electron surface, modules or IPC were produced.
- `inspect_analysis_view` on the scout's retained shell analysis (ev_07f3b307...): summary statistics (561 modules, 61 IPC operations, 33 literal channels, 28 main handlers, 11 context-bridge APIs including the wizard preload, 0 utility processes, 0 native addons, 33 sender-validation observations) and one module item (an `external:electron` reference, not useful).
- `trace_application_feature`: channel exact `shell:open-external` on the shell graph exceeded the MCP response budget (retained ev_c9c49e36...); string contains `/connect-websocket` on the UI graph returned no match because the semantic graph was truncated (direct grep confirms the literal).
- Not used: `open_binary`, `search_strings`, `list_strings`, `binary_overview` (avoided the shared active-target session; used read-only node scripts for strings instead), `inspect_pe_resources` (scout already ran it), `inspect_artifact`, `extract_artifact` (asar pre-extracted).

Other read-only methods
- Node scripts in `tmp/agarch/` to extract printable strings and Go symbol names from `language_server.exe`/`webm_encoder.exe`, locate and extract the embedded UI zip, dump embedded prompt/doc ranges, and read the Electron fuse wire from `Antigravity.exe` (`101100011`).
- PowerShell `Get-AuthenticodeSignature` on five binaries (all NotSigned).
- WebFetch of public docs: permissions, sidecars, browser (ide/browser), plugins, remote-control, getting-started navigation; the terminal-sandbox URL guess returned 404.
- Shell source read in full: `main.js`, `languageServer.js`, `hostBridgeServer.js`, `ipcHandlers.js`, `preload.js`, `customScheme.js`, `updater.js`, `utils.js`, `storage.js`, `settingsService.js`, `paths.js`, `constants.js`, `ideInstall/*`; `chrome-devtools-mcp` entry, browser, ToolHandler, McpContext, telemetry and key tools.

Files produced
- `findings/t3/antigravity.architecture.features.json`
- `findings/t3/antigravity.architecture.deep.md` (this file)
- Scratch (reusable by later agents): `tmp/agarch/ls_strings.txt`, `ls_docs_48M.txt`, `ls_go_pkgs.txt`, `ls_zip.130646139.txt`, `ui/` (extracted UI bundle), `webm_*.txt`, helper scripts (`strings.js`, `zipscan2.js`, `zipextract.js`, `ctx.js`, `dumprange.js`, `fuse.js`) and the record sources under `tmp/agarch/rec/`.
