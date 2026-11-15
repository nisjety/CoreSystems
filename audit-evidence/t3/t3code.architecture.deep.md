# T3 Code 0.0.35 - Architecture deep dive (Audit 3, facet "architecture")

Scope: shell and main process, provider integrations (how it drives Codex, Claude and other agents), session orchestration, local server and protocols, persistence, git and worktree handling, updates. Mapped to taxonomy categories 4 (Agent runtime), 6 (Computer and environment control), 9 (Models and routing) and 13 (Platform architecture), with records in other categories where the architecture carries them.

Companion file: `t3code.architecture.features.json` (94 records, ids `t3code-architecture-001` to `-094`). The sibling UI audit (`t3code.ui-and-multimodel.*`) covers the React UI, picker and timeline; this file does not repeat it and cross-refers to its record numbers where they overlap.

## 0. How to read this

Provenance tags: **SRC** = read in app-owned TypeScript recovered from the shipped `.map` files (`sourcesContent`), or in shipped config files; **REA** = observed through the REA reverse-engineering tools on a shipped artifact; **DOC** = in-source comments and help text; **INF** = my inference, basis stated. Confidence is "high" unless marked.

Evidence path roots (all under the audit scratchpad `SCR`):

| Root in evidence strings | Meaning |
|---|---|
| `recovered/server/src/...` | `SCR/tmp/t3code/recovered/server/src` - apps/server (about 345 files) |
| `recovered/server/packages/{contracts,shared,effect-acp,effect-codex-app-server,tailscale}/src/...` | workspace packages bundled into the server |
| `recovered/desktop/src/...` and `recovered/desktop/packages/{client-runtime,ssh,contracts,shared}/src/...` | apps/desktop (Electron main, about 72 files) and packages bundled into it |
| `tmp/t3arch/...` | files I derived: `src/serviceLauncher.ts` (extracted from `service-launcher.mjs.map`), `rea_ipc_channels.tsv`, `rea_trace_preload.json` |
| `ev_...` | REA evidence ids |
| `INSTALL:resources/...` | a file in the installed program folder (program file, not user data) |

Line numbers refer to the recovered file. Type-only modules (for example `provider/Services/ProviderAdapter.ts`) are erased by the compiler and are not in the source maps, so the adapter interface is inferred from its callers.

### 0.1 REA usage log (honest)

REA contributed measured facts about the desktop IPC surface and the native sidecar. Everything else comes from the recovered source, which is far richer than any bundle analysis.

- `analyze_javascript_application` (format directory, detail summary, record-and-continue, heap 2048 MB) on `extract/t3code/apps/desktop/dist-electron`: 12 relevant files, 8 parsed JS files, 170,657 AST nodes, 4 contextBridge APIs, 87 exposed API members, 101 IPC operations (all with identifier channel names, so literal strings are unresolved), 0 resolvable main-process handlers, coverage partial. Evidence `ev_8ebcd46d...` (view) over parent `ev_34f79ed7...`. [REA]
- `trace_application_feature` with an api seed `exposeInMainWorld`: no match (`ev_902fca7c...`, graph partial; not evidence of absence). With a module seed `preload.cjs`: 147 nodes, saved to a 437 KB file; from it I extracted the 101 channel operations (78 invoke, 4 sendSync, 5 send, 14 listeners), the 14 listener handlers and the 4 bridge APIs (`desktopBridge` with 86 members, `previewPictureInPicture` with 1, two Clerk bridges). [REA]
- Cross-check: all 83 `*_CHANNEL` constants in `ipc/channels.ts` appear in the preload; the 15 others belong to Clerk (token cache, OAuth transport, passkeys) and to the element-picker preload. [REA + SRC]
- `open_binary`, `binary_overview` (829 procedures, 531 strings, `ev_c06e6b80...`), `search_strings` (`ev_940e04a8...`), `close_binary` on `resource-monitor/t3-resource-monitor.exe`: Rust (sysinfo 0.39.3, serde_json), JSON command names and sample field names. The scout's `inspect_pe_resources` evidence is `ev_974d9254...`. [REA]
- Not used: `inspect_analysis_view` (the summary came back inline), `analyze_web_bundle`, `recover_javascript_sources`.

Tooling note: shell and node refuse to run with the long scratchpad path as working directory; all scripts were run from a short working directory with absolute paths.

### 0.2 Corrections to the scout inventory and the sibling audit

- Checkpoints: the scout wrote that checkpoint storage does not coordinate provider rollback. `CheckpointStore` does not, but `CheckpointReactor` does call `providerService.rollbackConversation` during revert (SRC `orchestration/Layers/CheckpointReactor.ts:779`). Claude's `rollbackThread` is lighter than Codex's (record 036).
- The resource monitor is a Rust program (REA, record 017).
- Telemetry default is enabled (`Config.withDefault(true)`, record 025).
- Port scanner has a Windows branch (PowerShell `Get-NetTCPConnection`, record 059).
- RPC serialization is JSON; `msgpackr` is not the RPC codec (record 006).
- Server secrets are plaintext `.bin` files protected only by permissions (record 020).
- Runtime modes are four, default full access (the sibling audit already noted this).

## 1. Executive summary

T3 Code is a **local-first agent-harness platform** wrapped in an Electron app. It does not call model APIs. It runs the user's own agent CLIs (Claude Code through the Agent SDK, Codex app-server, Cursor and Grok over ACP, OpenCode over its HTTP server) and normalizes their very different protocols into one event stream, one session model and one UI. Architecturally it is a **Node server (Effect 4 beta) that is hosted by many shells**: the desktop app, `t3 serve` headless, an SSH remote, a WSL distro, a systemd or launchd boot service, and a hosted web client that attaches to user-owned servers.

What it does best, and what Verevon can use:

1. **Event-sourced single-writer core with receipts and intent events** (records 008, 009, 010, 092). Commands are a closed schema; side effects run in reactors that react to `*-requested` events and answer with internal commands; read models are projectors with their own cursors; the sidebar stream attaches before snapshot and coalesces per aggregate.
2. **A provider SPI that survives five protocols** (031-040): drivers as plain values, instances as closures, a continuation identity so a thread can move between equivalent accounts, canonical runtime events that keep the raw native payload, and a guard against stale or out-of-order provider events.
3. **Coordinated rewind**: hidden git-ref checkpoints captured through a temporary index, plus a revert that restores files, rolls the provider conversation back and cleans refs (043). Verevon has no rewind today.
4. **A scoped MCP endpoint that every runtime can reach** (062, 063): per-session capability-scoped bearer, hashed at rest, fail-closed gating, injected through each protocol's own mechanism. This is the missing platform-tool path in the Verevon baseline.
5. **Collaborative browser control** (056, 057, 072): CDP plus an injected Playwright runtime for perception and locators, a reverse-RPC broker that lets an agent on any server drive a browser on the user's desktop with per-session host leases, and human-versus-agent arbitration that aborts an in-flight agent action when the human touches the page.
6. **Demand-driven background work** (019, 018, 017): client activity leases, power-aware policy and a Rust resource sidecar, so polling runs only for data somebody is looking at.
7. **Operational rigor for a self-hosted product** (004, 013, 015, 053, 054): fd-based bootstrap, readiness probes, fatal versus transient preflight failures, blue/green self-update with a database snapshot and automatic rollback, version-skew capability flags.
8. **Transparency by construction** (068, 069, 070, 079): dual-stream NDJSON logs of native and canonical provider events, local trace file plus OTLP, usage computed from transcripts, hot-reloaded config files with secrets split out.

Where it is weak relative to the references and to Verevon: single-user trust model (an administrative session can be minted by anyone with filesystem access to the state directory, record 064), plaintext server secrets (020), default runtime mode full access (071), telemetry on by default with identity derived from provider account ids (025), no memory (080), no research (081), no user-facing plugin or MCP management (088), the Claude rewind caveat (036), a permissive CSP (003), a process-local single-writer queue (008), and a dependency on a pre-release Effect (001).

## 2. System map

```
                         +------------------------------+
 hosted web client ----->|  Node server (apps/server)   |<----- t3 CLI (live mode or offline engine)
 (app.t3.codes, PWA)     |  Effect layers, Bun or Node  |
 phones via relay        |                              |
                         |  HTTP  : /.well-known/t3/environment, /api/auth/*,
                         |          /api/orchestration/{snapshot,shell,threads/:id,dispatch},
 Electron renderer ----->|          /api/attachments/upload, /api/assets/<signed>, /oauth/token,
 (t3code://app proxy)    |          /api/observability/v1/traces, /api/connect/*, /api/pull-requests/diff
                         |  WS    : /ws  Effect RPC (JSON, permessage-deflate), 105 method names, about 19 streaming
                         |  MCP   : /mcp  (per-session bearer; preview toolkit)
                         +-----+-----------+-----------+----+
                               |           |           |
          event store + projections    provider layer   native/aux processes
          (SQLite, WAL)                |                 - node-pty / Bun PTY terminals
                                       +- Claude Agent SDK -> user's Claude Code binary
                                       +- codex app-server (JSON-RPC over stdio), 1 per thread
                                       +- cursor-agent / grok agent stdio (ACP, stdio)
                                       +- opencode serve (HTTP, SDK client)
                                       resource monitor (Rust, JSON over stdio), git, gh/glab/az, ssh, tailscale, cloudflared
 Electron main (apps/desktop)
   supervises backend instances (primary, WSL), loads UI, owns native pieces:
   webview browser preview + CDP automation, SSH/WSL, updates, safeStorage, Clerk bridge, menus
   fd3 bootstrap, fd4 host telemetry, fd5 control, HTTP readiness probe at /.well-known/t3/environment
```

Protocol inventory (SRC):

| Link | Protocol | Notes |
|---|---|---|
| Renderer to server | Effect RPC over WebSocket (JSON) plus HTTP | `WS_METHODS` 97 plus `ORCHESTRATION_WS_METHODS` 8 (rpc.ts:209-334, orchestration.ts:27-36) |
| Renderer to Electron main | `desktop:*` IPC, schema-decoded handlers | 101 channel operations (REA), `desktopBridge` 86 members (REA) |
| Server to Claude | Agent SDK `query()` with an async-iterable prompt, `canUseTool`, `onUserDialog` | user's Claude Code binary |
| Server to Codex | JSON-RPC over stdio, generated client | 87 client requests, 8 server requests, 67 notifications in the schema; about 10 used |
| Server to Cursor and Grok | ACP v0.11.3 over stdio, plus vendor extensions | fs and terminal client capabilities off |
| Server to OpenCode | spawned `opencode serve`, `@opencode-ai/sdk` v2 | event.subscribe stream |
| Agents to server | MCP over HTTP at `/mcp` | one toolkit (preview, 14 tools) |
| Server to resource monitor | versioned JSON commands over stdio | Rust sidecar (REA) |
| Server to relay | HTTPS with environment-signed JWT proofs, cloudflared tunnel | pinned binary with SHA-256 |
| Desktop to server | fd 3 (bootstrap), fd 4 (host telemetry), fd 5 (control) | stdin for WSL |

## 3. The server core: data model and orchestration

### 3.1 Aggregates, commands, events (records 008, 009)

- Two aggregate kinds: `project` and `thread` (`commandToAggregateRef`, OrchestrationEngine.ts:63-81). Events carry `sequence`, `aggregate_kind`, `stream_id`, `stream_version`, `command_id`, `causation_event_id`, `correlation_id`, `actor_kind` and payload/metadata JSON (migration 001); a unique `(aggregate_kind, stream_id, stream_version)` index gives optimistic concurrency per stream.
- Commands come in three tiers: `DispatchableClientOrchestrationCommand` (what the UI may send), `ClientOrchestrationCommand`, `InternalOrchestrationCommand` (assistant deltas, session set, activity append, diff completion, revert completion). The decider is a pure function from (command, read model) to events (decider.ts, 1.4k lines; 31 command cases at lines 227-1354).
- The engine is one unbounded queue and one worker. For each command it checks the command receipt (conflict on aggregate mismatch, replay of accepted result, previously rejected), decides, stamps client origin onto the events, then in one SQL transaction appends events, projects them in memory and into tables, and upserts the receipt; it publishes to a PubSub afterwards (OrchestrationEngine.ts:109-260). On failure it re-reads persisted events to reconcile its in-memory read model and, for invariant errors, stores a rejected receipt (:287-325).
- Intent events: `thread.turn-start-requested`, `thread.turn-interrupt-requested`, `thread.approval-response-requested`, `thread.user-input-response-requested`, `thread.session-stop-requested`, `thread.runtime-mode-set`, `thread.meta-updated`, `thread.checkpoint-revert-requested`. Reactors consume them (ProviderCommandReactor.ts:53-70, 1452-1480; CheckpointReactor.ts:690-830) and report results through internal commands, so every effect is a durable, replayable fact.

### 3.2 Projections and sync (records 010, 092)

- Nine projectors (projects, thread messages, proposed plans, activities, sessions, turns, checkpoints, pending approvals, threads) with a per-projector `lastAppliedSequence` in `projection_state`; bootstrap resumes each from its cursor (ProjectionPipeline.ts:58-71, 1648-1770).
- Tables seen in the migrations: `orchestration_events`, `orchestration_command_receipts`, `checkpoint_diff_blobs`, `provider_session_runtime`, `projection_projects`, `projection_threads`, `projection_thread_messages`, `projection_thread_activities`, `projection_thread_sessions`, `projection_thread_proposed_plans`, `projection_turns`, `projection_pending_approvals`, `projection_state`, `auth_sessions`, `auth_pairing_links` (43 migrations; 011 and 016 rewrite event payloads in place).
- The live sidebar (`subscribeShell`) attaches a scope-bound buffer to the domain event stream before loading a snapshot, resumes from the client's sequence when it has one, coalesces events per aggregate with a keyed worker so bursts become one refetch, and emits upsert or removal from the refetched projection row (ws.ts:1322-1385, 640-760; KeyedCoalescingWorker.ts). This is the cleanest piece of the sync design.
- Streaming text is not persisted per token. Ingestion buffers assistant deltas (cap 24,000 characters) and flushes a message at completion; `enableLegacyTokenStreaming` flips to per-delta commands (ProviderRuntimeIngestion.ts:93-101, 1080-1215, 1680-1725) (record 042).

### 3.3 Startup and recovery (record 012)

Long-running roots are forked "parked" and released only at activation (serverActivation.ts). HTTP requests wait for command readiness (server.ts:439-453). At boot, `reconcileProviderSessions` finds thread sessions that claim to be starting or running but have no live provider session and marks them as error with a plain-language message and the binding as stopped (serverRuntimeStartup.ts:296-360).

### 3.4 Persistence layout (record 082)

Everything lives under one base directory: `userdata/` (state.sqlite plus WAL, settings.json, keybindings.json, attachments, logs, secrets 0700, environment-id, anonymous-id, server-runtime.json), `caches/`, `worktrees/`, and `runtime/versions/<v>` for pinned runtimes. SQLite runs in WAL with `busy_timeout=5000` because the CLI and the server write from separate processes (Sqlite.ts:37-40).

## 4. Provider integration: how T3 drives agents

### 4.1 Driver SPI (records 031, 032)

A `ProviderDriver` is a plain value: `driverKind`, `metadata`, `configSchema`, `defaultConfig`, `create(input)`. `create` returns a `ProviderInstance` (a record of closures: `snapshot`, `adapter`, `textGeneration`) with a `continuationIdentity`. No singleton Context tags exist per driver, so two Codex accounts share no state. The registry hydrates instances from settings, hot-reloads them and downgrades bad config to an "unavailable shadow" snapshot. Multi-account isolation is explicit: Claude instances use `CLAUDE_CONFIG_DIR` (not `HOME`, which would hide the macOS keychain); Codex instances can use an auth-overlay shadow home that symlinks shared state (sessions, skills, plugins, cache, logs) and keeps `auth.json` private. The continuation key (`claude:home:<path>`, `codex:home:<sharedHome>`) says which instances can continue each other's threads.

### 4.2 Adapters (records 035-039)

| Provider | Process model | Resume and rewind | Approvals and input | Interrupt | Notable |
|---|---|---|---|---|---|
| Claude | SDK `query()` in streaming-input mode; prompt is an `AsyncIterable` fed from an Effect Queue; one session per thread; `pathToClaudeCodeExecutable` is the user's binary | cursor `{threadId, resume, resumeSessionAt, turnCount}`; `rollbackThread` trims adapter bookkeeping only | `canUseTool`, `AskUserQuestion`, `onUserDialog` | hard stop of the query (SDK escalates to SIGKILL) | `settingSources` user/project/local; setModel and setPermissionMode applied live; a message sent mid-turn steers the same turn |
| Codex | one `codex app-server` child per thread; generated client | `thread/resume`, real `thread/rollback` | server requests held in pending maps | `turn/interrupt` | runtime mode becomes approvalPolicy + sandbox + approvals reviewer (auto uses `auto_review`); child agents registered from `thread_spawn` |
| Cursor | `cursor-agent` over ACP | `session/load` with replay gating | permission and `cursor/ask_question` extensions | `session/cancel` | plan and todo extension requests mapped to T3 plan events |
| Grok | `grok agent stdio` over ACP | same ACP runtime | xAI extension | same | |
| OpenCode | spawned `opencode serve` (or external URL with Basic auth) | `session.revert` | permission ruleset per runtime mode | SDK | `mcp.add` for the T3 server when T3 spawned it |

All five receive the same `t3-code` MCP server, each through its own mechanism (record 062).

### 4.3 Canonical events (records 040, 041)

`providerRuntime.ts` defines 48 event types, 16 item types, 10 request types and a `RuntimeEventRaw` envelope that keeps the native source, method and payload. Ingestion guards the lifecycle: a `turn.completed` that does not name the active turn is dropped (the Claude resume handshake emits a result with zero turns), `turn.started` conflicts with an active turn are ignored, and runtime errors only apply to the active turn (ProviderRuntimeIngestion.ts:1520-1580). The guard is on by default.

### 4.4 Session lifecycle (records 033, 034, 049)

`ProviderSessionDirectory` persists a binding per thread (instance, adapter key, status, resume cursor, cwd, model selection, runtime mode). `ProviderService.resolveRoutableSession` recovers a missing session on demand from that binding. A reaper stops sessions idle for 30 minutes (sweep every 5) unless there is an active turn or native background work. Approvals are callbacks inside the provider process, so after a restart a response targets a request that no longer exists; T3 reports this explicitly. Thread deletion stops the session and closes terminals but does not remove hidden checkpoint refs or worktrees (record 093, medium confidence).

### 4.5 Runtime modes (record 071)

One vocabulary: `approval-required`, `auto-accept-edits`, `auto`, `full-access`; default `full-access`. Translation: Claude permissionMode default/acceptEdits/auto/bypassPermissions; Codex untrusted+read-only, on-request+workspace-write, on-request+workspace-write+auto_review, never+danger-full-access; OpenCode ruleset; ACP mode aliases and auto-approval of allow options in full access. A mode change restarts the provider session.

### 4.6 Models, options and utility models (records 073, 091, 048, 016)

Catalogs are discovered per protocol: static Claude list gated by the installed CLI version, live paged `model/list` for Codex, ACP config options for Cursor and Grok. Effort, fast mode and thinking are descriptors applied to the live Claude session (`setModel`). Titles, commit messages and PR text use one-shot CLI calls with a JSON schema (`claude -p --json-schema`, `codex exec --output-schema`). A model manifest (1 h TTL) and the LiteLLM price table (24 h TTL) refresh from the web outside releases.

## 5. Environment control

- **Checkpoints and revert (043).** `refs/t3/checkpoints/<base64url(thread)>/turn/<n>` captured with `GIT_INDEX_FILE` set to a temporary index (read-tree HEAD, add -A, write-tree, commit-tree, update-ref). Restore runs `git restore --source ... --worktree --staged`, `git clean -fd`, `git reset`. The reactor then refreshes the file index, rolls the provider back by N turns, deletes newer refs and completes the revert.
- **Worktrees (044, 045).** `<base>/worktrees/<repo>/<branch>`, submodules initialised, `gh-merge-base` stored, missing directories recreated before every turn, first-message title and branch-rename tasks, optional start-from-origin. A single bootstrap command creates the thread, the worktree, runs the setup script and starts the turn, with cleanup on failure.
- **Terminals (055).** thread-scoped PTYs (Bun or node-pty), scrollback in per-terminal log files (5,000 lines), 1 s process-tree polling to show the running command.
- **Embedded browser (056, 057, 072, 058).** Webviews on persistent partitions; CDP debugger attached per webContents (refused if DevTools is open or another debugger is attached); Playwright's InjectedScript extracted from `playwright-core/lib/coreBundle.js` by string markers and evaluated in a VM to get locators; clicks go through `Input.dispatchMouseEvent` after an agent-cursor overlay event; snapshots return text, interactive elements, accessibility tree, console, network, action timeline and a PNG. The server-side `PreviewAutomationBroker` relays tool calls to the desktop that hosts the tab and leases a session to one host; a human input that does not match an expected synthetic input bumps a control epoch and aborts the agent's in-flight action while cleanup commands still run. Recording uses CDP screencast frames encoded in the renderer.
- **VCS and forges (060, 061).** A `VcsDriver` SPI with only git registered (`jj` is a declared kind), status broadcast with demand and failure backoff, four forge providers through the user's own CLIs.
- **Remote and WSL (053, 054).** WSL runs as a second backend with its own database; SSH launches or reuses a remote server through a stdin script, tunnels with `ssh -L`, and handles passwords through an ASKPASS helper.

## 6. Shell, updates and distribution

- **Main process (001, 003, 004, 005).** An Effect layer graph; a privileged `t3code://app` origin reverse-proxies to the backend and adds the CSP; backend instances are supervised with a restart loop (500 ms to 10 s), fatal versus transient preflight failures, SIGTERM then forced kill after 2 s; the pool supports a WSL instance beside the primary.
- **Desktop updates (014).** electron-updater with manual download and install, 15 s start delay then 4-minute polls, nightly versus latest, all backends stopped (5 s) before `quitAndInstall`.
- **Headless updates (013).** Pinned runtime directories, a staged preflight, a launcher that starts the new version as a trial against a snapshot of the SQLite files, commits when the trial reports prepared, and otherwise restores the snapshot and the previous version (tmp/t3arch/src/serviceLauncher.ts).
- **Distribution (085).** `app.asar` (91.6 MB) plus `server.asar` (130.5 MB), unpacked natives, the Rust sidecar and an elevation helper; GitHub releases feed.
- **IPC (028).** Schema-first `makeIpcMethod` registrations; 101 channel operations observed by REA.

## 7. Security and governance observations

Good: exhaustive scope table for every RPC (024); scoped bearer and DPoP sessions with replay protection (023); fd-based secret handoff (004); webviews sandboxed with an allowed-partition check (029); realpath checks at I/O sites (075); signed expiring asset URLs (076); per-session MCP credentials hashed at rest and revoked eagerly (062); pinned cloudflared hashes (022); guarded process kill with identity and ancestry checks (078); sensitive provider variables kept out of settings.json (079).

Weak: plaintext server secrets (020); permissive CSP (003); `/mcp` mounted outside the environment auth stack (062 notes); full-access default (071); telemetry on by default and identity read from provider account files (025); remote data used without authentication (the model manifest, the price table) and an unpinned triage playbook (077, 065); single-user model with admin sessions mintable from the filesystem (064); no org, role, audit or ZDR concepts anywhere.

## 8. Observability and transparency

Dual-stream NDJSON per thread (native and canonical, 10 MiB by 10 files, 512 MiB and 14 days caps, transient event types dropped from the canonical file) (068); a local trace file with optional OTLP and a browser-trace ingest proxy plus an in-app diagnostics reader (069); usage from provider transcripts at the base price tier with fork-copy discounting (070); a Rust process monitor with categories and history (017); a guarded kill switch (078); hot-reloaded settings and keybindings (079). This is the strongest transparency story among the apps in this audit for a local tool, and the "native versus canonical" pairing is the idea to take.

## 9. Best-in-class (relative to the references in the audit)

- Single-writer receipts and intent-event reactors (008, 009).
- Provider SPI with continuation identity and multi-account homes (031, 032).
- Collaborative browser control with host leases and human arbitration (057, 072): no reference app in this audit shows an equivalent.
- Self-update with trial and database rollback (013).
- Demand-lease background policy (019).
- Version-skew capability descriptor (015).
- Scoped, hashed, revocable MCP credentials (062).

## 10. Weaknesses and limitations

- Process-local state everywhere: the command queue, MCP credentials, broker leases, background leases and the PubSub are in memory. Fine for one user; each needs an external store for a hosted multi-replica service.
- Dependence on the user's agent binaries: every capability above the protocol (compaction, rewind semantics, plan mode) varies by provider, and T3 adds guards and special cases per vendor (the Claude adapter alone is 161 KB).
- Brittleness hotspots: scraping Playwright's bundle by marker string (056), ACP vendor extensions tracking private schemas (038), Codex child-agent registration marked WIP (047).
- Rewind caveat (036) and deletion leftovers (093).
- Event-store immutability broken by in-place payload migrations (011).
- Beta framework risk (Effect 4.0.0-beta.103 including unstable modules for RPC, HTTP API, SQL, process and MCP server).
- Windows specifics: `chmod` is a no-op, `t3 service` is unavailable, WSL has no resource telemetry.
- No evals, no scheduling, no memory, no research, no voice, no image generation (all absent in the recovered server).

## 11. What Verevon should copy, adapt or avoid

Reference points from the Verevon baseline (`findings/t3/v-modelplane.baseline.md`): Rust hot path with Go control, session-core as single writer on Postgres with durable approvals and encrypted continuation descriptors; two loops (read-only chat in the gateway, governed Do mode in execution-core); no rewind; MCP tools not reachable by either loop; agent browser runs fail closed because no grant is minted; process-local registries limit multi-replica; no ACP, AG-UI or A2A.

**Copy (adapt to Rust and Postgres):**
1. Command receipts with an aggregate-mismatch check and reconcile-after-failure in session-core (008).
2. Closed client-command schema plus intent events for approvals, interrupts, restarts (009).
3. Capability-scoped per-run MCP credential service (hashed at rest, liveness window, eager revoke, fail-closed gate) to give Do mode and chat a platform-tool path (062, 063).
4. Checkpoint and rewind for sandbox workspaces using Verevon's CAS manifests, with an explicit provider/conversation rollback contract (043, 036).
5. Canonical runtime event schema with raw-payload attachment and a lifecycle guard, as the prerequisite for hosting ACP and Codex runtimes (040, 041, 038, 037).
6. Generate Rust clients for the Codex app-server (and ACP) from the published JSON schema; use `turn/steer`, `thread/rollback`, plugin and skill methods that T3 leaves unused (027).
7. Orphan-run reconcile at boot, reaper that respects background work, and lazy resume from a persisted cursor (012, 033, 034).
8. Demand leases from the SPA to the gateway for watches, polling and provider health (019).
9. Version-skew capability descriptor for the SPA/gateway rolling deploys (015).
10. Dual-stream (native and canonical) run logs behind a debug view, subject to ZDR (068).
11. Browser control details for Quarry: accessibility-tree plus console/network/action-timeline snapshots, host leases, human-epoch interrupts (056, 057, 072).
12. Race-free sync recipe for the SPA stream: attach live tail, snapshot, coalesce per aggregate, refetch (092).
13. Compaction that asks the user when a stale large session resumes (046).

**Adapt carefully:** typed WebSocket RPC for the chat/agent page (006); runtime-mode vocabulary (071); settings with sensitive fields split to a secret store (079); repository identity keys (094); the bootstrap saga for "start a run in a Space" (045).

**Avoid:** plaintext secret files (020); full-access defaults (071); telemetry identity from provider account ids (025); remote prompt text or data without pinning (065, 077); admin sessions mintable from the filesystem (064); broad CSP (003); in-place event rewrites (011); a permissive no-scope operate role (024 notes).

**Rust portability (084):** the decider and projector, JSON-RPC stdio clients, projection tables, NDJSON logs, lease policy, DPoP and the launcher protocol port cleanly; Effect layers become traits and a supervisor; node-pty becomes portable-pty; the native file index becomes the ignore crate plus a matcher; the injected-script extraction and Electron/Clerk bridges have no direct analogue.

## 12. Open questions

1. Does a Claude revert really leave the model context unchanged? (036) Needs a runtime test of `resumeSessionAt` after `rollbackThread`.
2. What is the server side of the relay (the `t3_relay` endpoint kind)? Only the client contract and tunnel runtime are visible.
3. Does the server ever clean hidden checkpoint refs and worktrees for deleted threads outside the code read? (093)
4. How stable is the Playwright InjectedScript extraction across playwright-core versions? (056)
5. How much of the mobile app (Expo, in the monorepo) shares `client-runtime`? Not part of this build.
6. What does `serverPassword` in the settings schema gate? Name only was seen.
7. Does `app.t3.codes` (hosted client) require the relay or can it attach to a LAN server directly? The CORS layer suggests the latter.
8. Runtime behavior was not observed: the app was never launched; defaults such as keybindings, first-run flows and update cadence in practice are unverified.

## Appendix A. Record index

See `t3code.architecture.features.json`. Counts by category: Platform architecture 27, Agent runtime 14, Computer and environment control 13, Safety, permissions and governance 11, Models and routing 8, Tools, plugins and extensibility 8, Transparency, observability and control 5, UI/UX and product design 2, Coding capability 2, Chat experience 1, Multimodal and creation 1, Memory, learning and personalization 1, Research and web 1. Quality 5: 20 records; 4: 53; 3: 17; 2: 4. Uniqueness: unique 12, uncommon 52, common 30. Provenance: SRC 89, REA 3, INF 2.
