# Manus Studio 2.0.6 - how it controls a computer (deep dive, facet: computer-control-architecture)

Audit 3, read-only. Subject: Manus desktop 2.0.6 (Windows MSIX). Feature records: `manus.computer-control-architecture.features.json` (88 records, ids manus-computer-control-architecture-001..088). Taxonomy categories 6, 11, 13 and 4 are the focus; a few records sit in 5, 7, 8, 10, 12 where they belong.

Provenance tags: REA = observed through an REA tool run, SRC = read in shipped source or files (including binary strings), DOC = shipped documentation or UI copy, INF = my inference with basis stated.

Path legend used in the records: `extract/manus/` = pre-extracted app copy (dist/, node_modules/@next-agent TypeScript src, frontend/out); `install:` = the read-only install dir `C:/Program Files/WindowsApps/ManusAI.Manus_2.0.6.0_x64__vajzd2mq3s8wj`; `tmp/cu`, `tmp/bin`, `tmp/ui` = text I extracted into the scratchpad (embedded guides, binary strings, i18n keys). No per-user data directory was read. No secret-bearing file was opened. Nothing was executed.

## 1. Bottom line

Manus does not ship an agent in its desktop app. The cloud holds the planner and loop; the desktop (and the cloud sandbox) are **hosts** that execute typed actions. The strongest design ideas are:

1. One versioned host protocol (the "executor protocol") shared by the local operator and the cloud sandbox runtime. SRC: the operator embeds `manus-sandbox/sbx-go-svc/executor-protocol` and `mcphost`; enum `HostKind` has `OPERATOR` and `SANDBOX_RUNTIME`.
2. **Code-mode computer use.** The model gets one MCP tool, `computer-use/js`, and writes small scripts against a QuickJS-WASM sandbox with a persistent `state`, 64-call budget, output caps and structured partial-failure reports. The in-app browser uses the same runtime. SRC `computer-use-js.mjs`.
3. A schema-first browser contract (`manus.browser/v1`, 22 tools, 27 error codes) with an **exactly-once action ledger**, per-snapshot element refs, a five-level side-effect taxonomy, secret references bound to origins, and denial of plain-text input to password/payment fields.
4. A safety stack that is visible to the human: always-on-top overlay plus a Shift+Esc hard stop for Windows computer use; approval modes; folder-scoped grants; a Windows local-account sandbox with a canary-verified network block; remote control grants with expiring verification codes.
5. Remote screen control built on LiveKit (WebRTC) with a Rust publisher, **separate from** the semantic computer-use helper on purpose.

Weak spots: default-on privileged operator, individually unsigned Windows helper binaries, heavy and elevation-dependent Windows sandbox, two overlapping local-bridge generations (legacy sidecar and new operator), and much of the policy living only in model-facing prose and server-side code that the artifact does not show.

## 2. Architecture

```
                         Manus cloud (not in artifact)
   planner / agent loop / sandbox VM images / LiveKit rooms / connectors / policy
        |  executor protocol (protobuf over socket.io websocket, device token)
        |  ClientEnvelope{Execute, Cancel, SessionConfig, DeviceAddonConfig, HostCallResolved...}
        v
 +------------------ user machine: Manus Studio (Electron 43, MSIX) -------------------+
 | Electron main (JS)                                                                    |
 |  - spawns + supervises:  manus-computer-operator (Go)  <--named pipe JSON API-->  UI  |
 |  - IPC desktop-ipc-api (26 namespaces), trusted-URL gate                              |
 |  - in-app browser (WebContents + CDP) -> Browser MCP (127.0.0.1, bearer)  ---------+  |
 |  - screen-live bridge (random-token JSONL pipe) -------------------------------+   |  |
 |                                                                                |   |  |
 | operator (Go) children / peers                                                  v   v  |
 |   ManusComputerUseHelper (Rust, stdio MCP: UIA / SendInput / WGC)  <- QuickJS cu.* |  |
 |   sidecar (Go; legacy My Computer: FUSE-like fs + terminal over outbound WS)       |  |
 |   node-pty terminals, addon MCP servers (stdio/http), MCPB bundles                 |  |
 |   Windows sandbox runner (restricted token, ACL capabilities, WFP block)           |  |
 | manus-screen-live-publisher (Rust + livekit-ffi) --> LiveKit room --> viewer      |  |
 | manus-remote-input (Rust injector, JSON lines)  <-- controller data channel       |  |
 +---------------------------------------------------------------------------------------+
```

Evidence for the diagram: `dist/agentManager/operatorManager.js:319,518,564-595` (spawn, env, bridges), `dist/ipc/registry.js:131-139`, `node_modules/@next-agent/browser-runtime/src/BrowserMcpServer.ts:154`, `.../screen-live-service/src/electron/screenLiveBridgeServer.ts:68-95`, install `bin/build.json`. REA (analyze_javascript_application on `node_modules/@next-agent`, evidence id `ev_037f902f...a8412`) parsed 209 JS/TS files, found 2 BrowserWindow sites, 18 explicit webPreferences and 8 sender-validation observations; the retained graph is partial.

### 2.1 Where the agent runs (device taxonomy) - DOC/SRC
A task has one active device: the per-task cloud **Sandbox** (Linux, X11), a persistent **Cloud Computer** (also a Cloud Mac is sold), or one of the user **local computers**. The computer-use addon description encodes the rule: in the Sandbox with `sessionType x11` the desktop is the agent's own and needs no takeover consent; every other desktop (Cloud Computer, Wayland, the user's PC) needs an announced takeover and consent. Task device can be switched mid-task; Manus re-executes the task on the new device and warns about partial files (renderer i18n). Records 077 (device taxonomy), 019, 023, 025, 058, 075.

### 2.2 The executor protocol (strongest portable idea) - SRC (binary strings)
Recovered from `manus-computer-operator.exe` symbols: proto files `addon_job, common, device_addon, device_viewer, envelope, file, git, mcp_host, mcp, runtime_addon, screen_capture, screen_live, skill_catalog, terminal, transfer, wallpaper, workflow, workspace`. Client envelope: Hello, Execute, Cancel, ActionTransferAccess, DeviceAddonConfig, SessionConfig, HostCallResolved, RuntimeAddonCallAuth. Host envelope: Hello, ActionAck, ActionProgress, ActionCompleted, ActionFailed, ActionCancelled, HostCallRequested, DeviceAddonConfigApplied, SessionConfigApplied, TerminalCompletionOrphaned. Actions: File (16 variants), Git (PrepareWorktree, SwitchBranch, WorkspaceInfo), Mcp (12 variants), McpHostCall, ScreenCapture, ScreenLive (Start/Stop), SkillCatalog, Terminal (Open/Attach/Input/Exec/Resize/Signal/Snapshot/Close/Process), Wallpaper, Workspace (EnsureAutomaticWorkspace), DeviceViewer (Open/Renew/Close). Errors: AUTH_*, CANCELLED, COMMAND_LOST, CONFLICT, HOST_UNAVAILABLE, INTERNAL, INVALID_ARGUMENT, NOT_FOUND, PERMISSION_DENIED, RESOURCE_EXHAUSTED, TIMEOUT, UNSUPPORTED. Direction (cloud = client, host dials out) is INF from names and from `MANUS_OPERATOR_SOCKET_URL`. Limitation: symbol names only; raw descriptors are embedded gzip and were not decoded.

Why it matters: it is a control-plane contract with typed progress and cancellation, grants (ExecutionGrant, HostAccessGrant, UploadGrant, DownloadGrant, Multipart, StreamDownload), and a `HostCall` reverse channel (EnsureAgentTask, GetAgentTask, ListAgentTasks, JobGet/Progress/Complete/Fail/Pause) so local tools can call back into tasks.

### 2.3 Process and IPC model - SRC/REA
- Electron main to renderer: `desktop-ipc-api` (invoke), `desktop-ipc-event`, `desktop-ipc-meta`; only senders on `app://manus`, `app://frontend` (and localhost in dev) or registered documents are served (`dist/ipc/registry.js:131-139`, `dist/utils/trustedUrls.js:7-12`). Preload exposes three bridges.
- Electron to operator: named pipe `\\.\pipe\manus-operator-localapi-<uuid>` (fd 3 on unix), JSON request/notification with 10 s default timeout, 4 MiB buffer, and an "outcome unknown" error class plus `deadlineUnixMs` for mutating calls (`dist/agentManager/operatorLocalApi.js:15-16,84,132`).
- Operator to cloud: socket.io websocket with a device token obtained from `/api/desktop/device-token`; revoked on logout (`operatorManager.js:452-500`).
- Operator to browser: MCP Streamable HTTP on 127.0.0.1, random port, 32-byte bearer token, timing-safe compare (`BrowserMcpServer.ts:101,154,1032`).
- Operator to screen-live: random-token JSONL over a pipe or 0600 socket, 64 KiB line cap (`screenLiveBridgeServer.ts:13,68,95,162`).
- Operator to computer-use helper: MCP stdio, scripts run through the QuickJS runtime (JSON lines, 1 MiB cap) (`computer-use-js.mjs:38`).
- Human viewer to publisher: LiveKit data topics and RPC (`manus.screen_live.*`).
- Legacy My Computer: socket.io `/device` namespace, versioned desired state, sidecar per session (`myComputerService.js`).

## 3. Computer use (agent controls the PC)

### 3.1 Tool protocol: code mode - SRC
- Single MCP tool `computer-use/js` (addon.json). Inside the cell the model calls `await cu.<native_method>(args)`; native methods on Windows: list_apps, open_app, get_app_state, find_elements, click, drag, scroll, press_key, type_text, set_value, select_text, perform_secondary_action, move_pointer, computer_use_status, computer_use_finish (helper strings). macOS adds read_element and an app-bound `cu.app` API.
- Runtime: QuickJS WASM 0.32.0, 64 MiB heap, 512 KiB stack, no Node/fs/net/imports; limits 64 native calls, 38 KiB output, 3 images, default 30 s (max 120 s); `state` persists across cells; reset on timeout, cancellation, operator restart, release or 30 min idle (`computer-use-js.mjs:20-21,104,246,273,476,557`; embedded guide at line 2).
- Errors carry `code`, `method`, `callIndex`, `nextAction`, `progress` (completed phases and call indices) and the host reports `statePreserved`. `cu.phase({name, calls, reserve}, fn)` pre-checks the budget and throws `PHASE_CALL_LIMIT`.
- Results are `{text, images, isError, callIndex, structuredContent}`; macOS compacts metadata (`[computer_use.meta/v1]` first line).
- Same runtime and rules power the in-app browser SDK `iab` (`in-app-browser-js.mjs:481,526,831`; `browser-runtime/src/javascript/guide.txt`).

Best-in-class: bounded script cells with budget and resume semantics. Weakness: efficiency is asserted by design, not measured; debugging a model-written script is harder than reading tool calls.

### 3.2 Helper and observation model - SRC
Rust helper (7.0 MB, rustc toolchain, same source revision as the screen publisher). Windows: UI Automation tree, SendInput, Windows Graphics Capture and PrintWindow fallbacks, DWM attributes, SetWinEventHook. Observations are semantic: short `element_index` handles tied to the latest `get_app_state`/`find_elements`, optional screenshot with `screenshot_id`, pruning flags (`semantic_pruning`, `interactive_only`). Targeting ladder: HWND, PID, AUMID, exe path, exact title; ambiguous names refused. Coordinate actions must pass the displayed raster size (`viewport_width/height`) and `screenshot_id`; stale or covered targets are rejected ("point is covered by another window even after raising the target"). REA `inspect_pe_resources` on the helper returned zero resources: no manifest, so no uiAccess; it runs at the user's integrity and skips activation of higher-integrity targets.

### 3.3 Honest foreground semantics - SRC/DOC
The Windows guide says which calls always activate (click, drag, move_pointer, Invoke/Toggle/Expand/Select actions) and which can stay in the background (reads, find, UIA-pattern set_value, select_text). A background state read does not prove background input. This is rare candour and good for trust.

### 3.4 Playbook - DOC
"Observe, plan, batch, verify": finish once, text-only observations by default, no discarded observations as waits, repair rather than replay, stop at sufficient evidence, user stop and permission denial are terminal. Safety gates: app text is untrusted; ask immediately before destructive, representational, financial, permission, install, upload or sensitive-data actions; never bypass UAC/secure desktop.

### 3.5 Linux and macOS - DOC/SRC
Linux: AT-SPI, X11 (own sandbox only) or Wayland xdg Portal with explicit monitor authorization; never fall back to X11/XWayland; window-capture authorization is read-only. macOS: signed service/client app pair with team-id allow-list; Accessibility and Screen Recording permission guides.

## 4. Browser control

Three surfaces (UI strings): cloud browser, **In-App Browser** (Electron, dedicated profile, imported cookies/passwords/passkeys), **My Browser** (user's Chromium via the Browser Operator extension, per-tab authorization, fallback to default browser).

In-App Browser driver (SRC/REA): Electron `webContents.debugger` (CDP 1.3), `Accessibility.getFullAXTree` plus `DOMSnapshot.captureSnapshot`, OOPIF targets, isolated worlds, `DOM.getContentQuads` for geometry. Snapshot pages are capped at 12,000 serialized chars with a 50,000-node budget; refs are scoped to `(snapshotId, ref)` and a document revision; coordinates are bound to a capture id and the exact raster size.

`manus.browser/v1` (SRC `browserContract.ts`): 22 tools, JSON Schema 2020-12, MCP annotations, `BROWSER_CONTRACT_GOLDEN` for contract tests, 27 error codes with `retryable` and `requiresSnapshot`.

Exactly-once effects (SRC `BrowserMcpServer.ts:248-340`): every call carries `_meta ai.manus/sessionUid` and `ai.manus/actionId`; for tools not classified read/read_sensitive the server keeps a ledger entry keyed by `len:sessionUid+actionId` with a SHA-256 digest of canonicalized arguments for 10 minutes; replay returns the first result, different payload gives `ACTION_REPLAY_CONFLICT`, unknown outcome gives `ACTION_IN_DOUBT`. The server states the Go operator journal owns cross-process durability.

Secrets and payments (SRC `BrowserDriverTextInput.ts`, `BrowserMcpServer.ts:706-790`): `{{secret:<name>.<field>}}` references substituted by the operator; origin allow-list normalised with fail-closed parsing and checked against both top-level and target frame; `SENSITIVE_INPUT_DENIED` for plain-text agent input to password/payment fields; one-time card fields through a masked temporary-input channel; Stripe Link one-time cards exported to an owner-only directory (operator `builtincli`).

Workspaces (SRC `base.ts:163-175`): per-task tab workspaces, agent tab limit 8, retained 30 min, max 10 retained sessions, restore after restart, background sessions never steal the visible surface; `TAB_CLOSED_BY_USER` stops the model reopening pages the user closed.

## 5. Remote screen streaming and human takeover - SRC

Start flow: cloud action `ScreenLive.Start(url, roomName, publisherToken)` -> operator -> authenticated JSONL bridge -> `screenLiveService` -> `ScreenLiveRustRuntime` spawns the Rust publisher (livekit-ffi, libwebrtc tag `webrtc-89d790b`, `customWebRtc=false`). One track per display (`screen:<n>`), metadata/attributes for platform, `desktopActions`, input availability. Region cache probes LiveKit regions. Session ends on `stopped`, `disconnected` or `no_viewer_timeout` (30 s join window). Power handling: display wake, power-save blocker, macOS keeps capturing the lock screen; Windows/Linux suspend until unlock then resume.

Human control: a participant is a controller only if identity starts with `controller-` and has `canPublishData`; input also requires live capture and granted OS permission. Normalized [0,1] coordinates bound to a track; strict bounds (scroll <= 8, text <= 16,384 chars, modifiers <= 8, epoch+sequence on cursor events); a **separate** Rust injector (`manus-remote-input`) with a generation counter, `release_all`, and statuses including `admin_window`. Source comment states the injector deliberately does not reuse the computer-use helper because that is a semantic engine that cannot support human passthrough. Clipboard/file paste, sound RPC, cursor sync with capture exclusion. Wallpaper-only mode reads OS wallpaper settings, never pixels.

Consent UX (i18n): remote control request with expiring verification code, approval once per device, revoke lists, "Pause control", "Take control", "Exit takeover". Strings only; cryptographic binding of code to grant is not visible.

## 6. Local filesystem, terminals, workspaces

- Legacy My Computer: sidecar mounts one shared folder into the cloud sandbox over an outbound websocket (FUSE-like ops: read/write/readdir/stat/chmod/link/symlink/readlink/rename/remove/truncate/utimens/mkdir), `pathguard.Resolve` blocks escapes, `parentwatch` exits with parent; terminals only in folders marked `allowCmd` (`myComputerService.js:96-110`).
- Operator path: automatic dated workspaces (`Documents/Manus/<date>/<id>`), session bindings, file roots, watcher, git status reporting, worktree retention with preview-then-commit cleanup and recovery guides.
- Terminals: local node-pty (ConPTY) with an 8 MiB unattached buffer and caps (128 total, 64 per owner); remote terminals over socket.io with per-event sequence numbers and `afterSequence` resume; `authorizationVersion` fence.

## 7. Permission and safety model (layers)

1. Device authorization and identity: device-bound login, device token, revoke on logout/removal, copied credential detection.
2. Approval modes per task: `ask_for_approval`, `approve_for_me` (reviews and auto-approves safe local actions), `full_access` (renderer code). Approval cards carry command, cwd, `declaredNetwork`, `writablePaths`, `requireEscalated`, `flaggedReason`; actions Allow once / Allow for this task / Always allow / Deny. Executor sandbox modes `WORKSPACE_WRITE`, `DANGER_FULL_ACCESS`, `ISOLATED`, `SHARED`.
3. Enforcement: Windows "safe modes" (local sandbox account, restricted token, per-root capability ACLs, persistent WFP outbound block, canary processes, NTFS only, elevation to set up); Linux bwrap (bundled in AppImage); macOS denial inference; managed Cloud PC runner with private desktop and window station per session.
4. Folder grants: per task, per folder, separate `allowCmd`; copy states a grant never widens network or cross-task access.
5. Secrets and payments: reference substitution with origin binding, sensitive-field denial, one-time cards.
6. Visibility and stop: safety overlay and Shift+Esc stop for computer use; activity indicator; user stop terminal.
7. Information flow: session/tool/connector data classification with no-write-down approval; privacy taint (sessions that used My Computer cannot be collaborative).
8. Network policy: org baseline plus project and risk policy; outbound-only tunnel client for private MCP servers; local-network access blocked unless tunnelled.
9. Process hygiene: operator env whitelist, log redaction, per-launch random bridge tokens, single-use file-grant tokens, trusted-URL IPC gate.

## 8. State sync and persistence

Desired-state reconciliation for devices (versioned `device_config`, applied-version `device_status`, 200 ms coalescing, 10 s grace); level-triggered status; operator epoch recovery (low confidence); worktree recovery records; terminal sequence resume; browser workspace restore; per-addon JSON storage; `userData` files (paths only, contents not read).

## 9. Best-in-class, weaknesses, copy or avoid

Best-in-class: executor protocol; code-mode cells with budgets; exactly-once action ledger; schema-first browser contract with golden test; secret references with origin binding; separate agent/human engines; Shift+Esc stop with overlay; honest foreground semantics; canary-verified egress block; privacy taint on collaboration; remote control approval with expiring codes.

Weaknesses and risks: operator enabled by default (`operatorManagerUtils.js:4-12`); six helper binaries NotSigned (Get-AuthenticodeSignature) so trust relies on MSIX and path ACLs; orphan sweep uses PowerShell with `-ExecutionPolicy Bypass`; user terminal inherits the full environment; two overlapping bridge generations (sidecar and operator); Windows sandbox needs administrator setup and NTFS; much consent behavior is model-facing prose (Linux takeover announcement) rather than enforced code; `begin_maintenance_v2` has no caller in dist; default Windows computer use moves the real cursor and foregrounds windows (no background desktop).

Copy for Verevon: (1) a typed host protocol shared by sandbox lease, governed browser and any endpoint; (2) side-effect enum plus idempotency ledger at the Quarry boundary; (3) secret references with origin binding and fail-closed host parsing; (4) error catalogue with retry flags and `nextAction`; (5) a bounded code-cell tool with budget and `statePreserved` for Do mode; (6) declared-permission approval cards; (7) monotonic data classification and privacy taint; (8) canary-verified egress blocking and denial reports for bwrap; (9) desired-state sync for leases and grants; (10) per-reason rejection counters as audit events; (11) own-sandbox versus foreign-desktop consent split; (12) two engines for agent and human control if a takeover view is built.

Avoid: default-on privileged local listener; unsigned helpers; Windows local-account sandbox complexity; bulk import of user cookies/passwords; letting the model decide consent in prose only.

Fit with Verevon rules (from baselines): "Model proposes, Quarry executes" is preserved if the Manus operator is mapped to Quarry/endpoint and the Model Plane only emits typed actions. OS computer use and own-Chrome are parked by the owner; the useful near-term lift is browser-side (records on ledger, secret refs, takeover, contract). ZDR must propagate: use the privacy-taint idea; Space access never widens: mirror the "grant does not widen X" copy.

## 10. Open questions

- Field-level schema of the executor protocol (decode the embedded proto raw descriptors).
- How `approve_for_me` decides (model reviewer versus rules) and how `flaggedReason` rules are authored.
- Whether Windows computer use can run on a private/hidden desktop (the Cloud PC runner uses one; the user-desktop helper does not appear to).
- Cryptographic binding between remote-control verification code and the LiveKit controller identity.
- Server-side enforcement of data classification and network policy.
- Whether the legacy sidecar is deprecated.
- Real-world latency and cost of code-mode versus per-action tools.

## 11. REA usage log and limitations

REA tools really used: `analyze_javascript_application` (1 heavy run, format=directory on `node_modules/@next-agent`, record-and-continue, 209 files, 1,159 findings, evidence `ev_037f902f240e6a58ccbcd22c96fc3f5305bcd51db95596610770ecf1939a8412`); `inspect_analysis_view` (modules page, offset 0 limit 120 of 740 coverage); `trace_application_feature` (module seed `screenLiveBridgeServer`: 3 seeds, 740 nodes, 884 edges; endpoint nodes were DOM attribute reads and are not used); `inspect_pe_resources` on the Windows helper (evidence `ev_bb7ad36929bdcc486499e45c40987816cce9a38006af6686b5ed3db464ef05a1`, zero resources). The scout's earlier REA run on `dist/` was not repeated. Not used: `open_binary`/`binary_session` (avoided to keep the shared REA active target untouched), so Go and Rust binary claims rest on string extraction (SRC), not REA.

Limitations: cloud-side code (agent, Sandbox VM, LiveKit rooms, policy engines, extension) is absent; the renderer is minified (UI behavior from i18n keys and a few code windows); Go and Rust helpers were not run or decompiled; Windows consent flow before computer use starts is not visible beyond the overlay; no per-user data was read.

## 12. Record index by section (feature ids end with these numbers)

- Architecture and protocol: 001 cloud brain/local hosts, 002 executor protocol, 003 process model, 004 wire formats, 005 per-launch credentials, 006 device config reconciliation, 007 supervision, 008 local API, 009 Rust-portable tier, 010 distribution, 078 file transfer plane, 088 capability flags.
- Computer use: 011 native helper, 012 code-mode tool, 013 cell budgets, 014 playbook, 015 targeting, 016 coordinates, 017 foreground model, 018 action receipts, 019 Linux/Wayland rules, 020 safety overlay and Shift+Esc, 021 visible pointer, 022 safety gates, 023 device switching, 024 keep-awake, 077 device taxonomy, 082 screen capture action.
- Browser: 025 three surfaces, 026 CDP driver, 027 contract, 028 snapshots/refs, 029 action ledger, 030 side-effect taxonomy, 031 secret references, 032 sensitive-field denial, 033 Stripe Link cards, 034 untrusted content, 035 workspaces, 036 iab SDK, 037 browser surface features, 038 data import, 039 network policy, 083 My Browser authorization, 084 human takeover for login.
- Remote screen and human control: 040 screen live, 041 controller authorization, 042 input protocol, 043 two engines, 044 clipboard/sound, 045 cursor sync, 046 wallpaper view, 047 remote control grants, 074 UI vocabulary.
- Filesystem, terminal, workspace: 049 sidecar, 050 operator workspaces, 051 folder grants, 052 terminals, 053 worktrees, 054 device services, 087 webdev addon.
- Sandbox, approvals, governance: 055 permission modes, 056 approval cards, 057 Windows safe modes, 058 Cloud PC runner, 059 data classification, 060 privacy taint, 061 env whitelist/redaction, 062 IPC trust and file grants, 063 helper integrity, 064 device identity, 065 default-on operator, 086 portal token storage.
- Agent runtime and extensibility: 029 ledger, 066 host calls, 067 workflow journal, 068 maintenance fence, 069 queue/interrupt, 081 epoch recovery, 070 addons, 071 MCPB, 079 skill catalog, 080 MCP auth states, 048 skill recorder.
- Observability and product: 072 tool credit metering, 073 diagnostics, 075 Cloud Computers, 076 capture/voice, 085 replay.
