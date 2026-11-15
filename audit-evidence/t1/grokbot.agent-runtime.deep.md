# Grok Bot 0.70.2 - agent runtime deep dive (facet: agent-runtime)

Audit 1, Verevon "Space". Source: Grok Bot desktop agent 0.70.2 (SpaceXAI), Windows x64 Electron 42.1.0 build, internal package name "sand". Companion files: `grokbot.agent-runtime.features.json` (147 records, ids `grokbot-agent-runtime-001..147`) and the earlier scout file `grokbot.inventory.md`. Record references below look like [R042] and mean the record with that numeric suffix.

Provenance tags used throughout: **REA** = observed through REA analysis of the shipped artifact, **SRC** = read directly in the shipped bundles or `package.json`, **DOC** = documentation (none ships with the app), **INF** = my inference with the basis stated. Path shorthand for evidence: `D` = `dist/local-exec-daemon/main.cjs`, `C` = `dist/node-agent-coordinator/main.cjs`, `MC` = `dist/electron-main/main-core.cjs`, `MA` = `dist/electron-main/main-app.cjs`, `PX` = `dist/electron-main/proto.cjs`. Offsets in the JSON evidence are character offsets into the minified bundle plus the line number.

## 0. Method, resume note and honesty section

- This is a resumed run. The interrupted attempt left a valid 114-record JSON and no deep dive. I re-read the JSON, re-checked every evidence anchor by script (all 537 anchors in the final file map to the stated line number; 0 mismatches), and read the source text behind 12 older records (R001, R009, R016, R021, R023, R028, R042, R044, R059, R062, R087, R102) plus every new record to confirm the claims. The older records held up. I rewrote one thin record (hooks, [R039]), appended text to two ([R049], [R100]) and added 33 new records (R115-R147). [SRC]
- REA tools really used. Earlier run: `analyze_javascript_application` on the whole `app.asar` (616 files, 5,217 findings, partial coverage), `inspect_analysis_view` (summary and module views), `trace_application_feature` (4 seeds, all no-match on a partial graph), `inspect_pe_resources` (Grok Bot.exe and the WebAuthn signer). This run: `analyze_javascript_application` on the extracted `dist/electron-preload` directory (5 files, 6 context-bridge APIs, 9 IPC operations of which 7 have dynamic channel expressions; evidence `ev_89468eaf...b193a`), and `trace_application_feature` on that evidence (api `ipcRenderer`, no-match). Two calls against the old retained evidence failed with `evidence_integrity_mismatch` because REA no longer retained it; I did not fake results from them. REA's static graph is partial, so a no-match is not evidence of absence. [REA]
- Everything that shapes the agent is mostly **server side** (the agent loop, prompts, memory algorithms, the Box image). The client shows the contract (typed protobuf messages, tool names, settings, gates), not the behaviour behind it. Where I describe server behaviour it is [INF] from message shapes and I say so.
- A Statsig client key and a Sentry DSN exist as literals in the bundles. They were deliberately not recorded. No per-user data stores, cookies, token files or `.env` files were read ("secret-bearing file skipped").

## 1. Executive summary

1. **Brain in the cloud, hands on the machines.** The agent loop runs server side (per-user "Box" harness, or a durable Temporal harness for rooms, [R004]). The desktop app is a thin host plus an *exec daemon* that performs tool calls on the user's own computer through a typed protobuf protocol with about 37 argument kinds, cancel, heartbeat and W3C trace context [R001, R002]. The same protocol works for the user's Mac, a phone, and the cloud Box. [SRC]
2. **The agent gets its own PC as a managed cloud computer** (the Box): lifecycle, hibernation, wake-on-intent, presence-aware parking, hand-back, per-team break-glass [R005-R008, R034, R072, R101]. Pixel-level desktop control (X11), a live noVNC view for the human, a polished screen-recording pipeline, and identity portability (Chrome cookie import with per-origin approval, passkey proxy to the user's key, an egress tunnel that exits through the user's own network) [R018-R023, R129]. [SRC]
3. **The tool contract is excellent.** Every tool call has a typed result union with named failure variants (rejected, permission denied, not found, busy, timeout, OOM kill, sandbox unsupported, ...) [R126]; shell output is spilled to files with head/tail/elided counts [R122]; the shell feels stateful through snapshot-and-replay [R123]; files, MCP tools, transcripts and notes are all exposed as paths [R115, R121]. [SRC]
4. **Permission model is the most mature part:** per-machine never/ask/always with a team ceiling, standing grants scoped to action or recipient, approval text written for the model, an LLM auto-review with natural-language allow/block rules, AST prechecks, ten hook events with allow/deny/ask and input rewriting, a pluggable permissions service with typed block reasons, native sandbox policy [R009-R013, R039, R059-R065, R125]. The big gap: **no filesystem sandbox on Windows** [R061]. [SRC]
5. **Realtime/state design is production grade:** one multiplexed transcript watch stream with per-agent cursors and generations [R044], an idempotent send journal [R045], a coordinator utility process behind a MessagePort [R046], a named policy object for almost every wait, retry or debounce [R049], and explicit async resource states [R134]. [SRC]
6. **Telemetry is measured against what users feel** (114 named events: first render, send-ack, lost rows, parity) and is consent-gated, bounded and self-auditing [R136, R144, R147]. Flags come through the product backend rather than a vendor SDK [R135]. [SRC]
7. **It is a re-skinned coding-agent platform.** Packages, protobuf namespaces, hosts, hook names, plugin manifests and skill folders are the Cursor agent stack plus Claude and Codex compatibility [R119, R120, R140]. That is why the harness is so typed, and it is a strong signal that a general harness plus a few product layers is a viable architecture. [SRC for names, INF for interpretation]
8. **For Verevon:** the highest-value, lowest-conflict adoptions are the typed tool-result envelope, the permission-reason taxonomy, MCP-as-files and filesystem-as-context for the sandbox workspace, shell output spill with elision counts, the stale-while-revalidate and supersede resource contract, consent-gated telemetry, and an agent-definition/memory-shard data model. The highest-risk items (agent-owned cloud PC with login portability, agent self-administration, cookie import) collide with Verevon's rules (Quarry executes browser actions, decisions come from outside the agent, ZDR must propagate) and need a redesign, not a copy (section 11).

## 2. Architecture map

### 2.1 Process topology [R076, R143, REA]

Five kinds of process plus native helpers:

- **Main process** (`MC` 854 KB core, `MA` 2.58 MB app wiring): windows, account state, auth, experiments, analytics, MCP manager, update, gateways, safe storage. Boots in two stages with a V8 code cache keyed by build id [R077].
- **Renderer(s)**: React 19 UI, no Node. REA counts 4 browser windows, 6 context-bridge APIs and 15 IPC operations across the app (13 with dynamic channel expressions); the preload directory alone has 9 operations (7 dynamic). The main preload exposes only `desktop`, `coordinatorPort`, `sandRenderTrace` and `desktopMount`. [REA, R143]
- **Coordinator utility process** (`C`, 882 KB): owns all network sync with the backend (roster, transcript tail, sends, automations), the WebAuthn and cookie-approval providers and the voice harness; reached from the renderer over a MessagePort carrying typed RPC with capabilities and failure codes [R046, R047]. [SRC]
- **Local exec daemon** (`D`, 3.5 MB): the "hands". Shell, files, MCP, computer use, recording; connects *out* to the cloud over SSE and receives requests there [R042]. Supervised by the main process with discovery file, adopt-or-replace and respawn limits [R050]. [SRC]
- **Native helpers**: `sand-webauthn-signer.exe`, a sandbox helper (macOS/Linux), a Chrome cookie import worker, tree-sitter-bash, `cursor-proclist`, notification-settings. [SRC, REA]

Why it matters: the model never talks to the renderer, and the renderer never touches the network directly. Every side-effecting path crosses a typed boundary.

### 2.2 Two agent harnesses [R004]

Agents are created either as **box** agents (the per-user Box harness, host-mediated) or **temporal** agents (server-owned, durable; required for multi-agent rooms). Capability flags (`durable_identity_enabled`, `temporal_creation_enabled`, `server_rooms_enabled`, `agent_messaging_enabled`) and a migration rollout status message show a live migration from the first to the second. A server-orchestrated room turn is a request with nonce, peers, new messages, deadline and a wind-down flag, answered by a delivered result [R028]. [SRC]

### 2.3 Network planes [R042, R043, R139]

- **Connect RPC (protobuf)** over HTTP/2 to seven `aiserver.v1` services, about 480 methods (GrokBotService 309, DashboardService 73, SandBoxService 55 largely duplicated, AgentStoreService 24, AiService 8, AnalyticsService 8, BackgroundComposerService 7). A very large single service is both a map of what an agent product needs and a cautionary example. [SRC]
- **SSE reverse channel**: the daemon opens `GET /local-exec/requests` (event stream) and posts frames to `/local-exec/responses`; no inbound ports; hello and welcome frames, heartbeat every 10 s, stall watchdog at 35 s, backoff 1 to 10 s, outbox flushing in batches. [SRC]
- **Per-account channels in one daemon** plus a Box gateway [R043]; HTTP/2 ping and pool tuning [R058].
- **noVNC/RFB** for the human's live view of the Box desktop [R020]; a webview preload and a VNC preload carry browser-specific channels (passkey stalled, popup closed, origin return). [SRC]

### 2.4 Cursor platform lineage [R140]

`@anysphere/*` workspace packages, protobuf packages `aiserver.v1` and `agent.v1`, `*.cursor.sh` hosts, `CURSOR_*` environment names, `.cursor` folders, a "CURSOR MANAGED BLOCK" in `~/.cursor/.gitignore`, hooks with Cursor and Claude names, plugins in `.cursor-plugin` or `.claude-plugin` manifests, skill roots for `.cursor`, `.claude`, `.codex`, `.grok` and `.agents`. Shims for external harnesses (pi, mini-SWE-agent) [R038]. [SRC] Interpretation [INF, high]: Grok Bot is the Cursor agent harness re-skinned for personal and team assistants, with a cloud computer, presence, rooms, a vault and messaging channels added around it.

## 3. The exec protocol and the tool contract

### 3.1 Wire shape [R001, R002]

`ExecServerMessage` carries an id, exec id, optional machine id and one of ~37 argument cases (shell and shell_stream, background_shell_spawn, write_shell_stdin, read and redacted_read, write, delete, ls, grep, diagnostics, request_context, mcp plus resource and state calls, fetch, record_screen, computer_use, execute_hook, subagent plus await and force_background, smart_mode_classifier, canvas_diagnostics, three allowlist prechecks, git_diff, conversation_search, agent_store_conflict, pi and mini-SWE shims). `ExecClientMessage` mirrors each case as a result and adds local execution time and hook contexts. Control frames: stream_close, throw, heartbeat from the client; abort from the server. Every args message carries `tool_call_id`. Handlers register per DI token and run as cancellable async generators, so streaming and cancel are uniform. [SRC]

Best-in-class aspects: typed everywhere, cancellable, traced, machine-addressable. Weak aspect: the *user computer* transport wraps these in JSON strings (`message_json`) inside the Connect frames, so protobuf typing is lost on that hop [INF from `GrokBotUserComputerClientMessage { message_json }`].

### 3.2 Tool names are a contract [R003]

One enum of 117 model-facing tool names is shared by daemon and host; approval and refusal copy is built from it. The families: shell and files; computer and screenshot; browser secure fill and credentials; machines (ListMachines, CopyToBox, CopyFromBox); memory and context; subagents; routines and tasks; documents and canvases; MCP and plugins; email, Slack, Teams, iMessage; meetings and phone; payments; agent administration (CreateAgent, UpdateAgent, SetPrimaryBot, PublishToTeam, ConvertToTeam). [SRC]

### 3.3 Typed results and named failures [R126]

Examples: ShellResult {success, failure, timeout, rejected, spawn_error, permission_denied}; ShellFailure adds `abort_reason` and `oom_kill {kind, memory_limit_bytes}`; DeleteResult has `file_busy` and `not_file`; ReadResult has `invalid_file {reason}`; McpResult has `tool_not_found` and `server_not_found`; SubagentAwaitResult has `still_running`. The model gets a cause, the UI can render a cause-specific card, and tests can assert causes. [SRC] This is the single most portable idea for the Model Plane tool contract.

### 3.4 Shell runtime [R014, R015, R016, R122, R123, R141]

- Shell flavours zsh, zsh-light, bash, powershell, naive; on Windows the daemon locates Git Bash and converts `C:\` paths to `/c/`.
- **Stateful over stateless**: each command runs in a fresh process, but after each command a dump script prints cwd, exported variables, options, aliases and functions between markers; the next shell evals that snapshot. The command is delivered through file descriptor 5, not argv. Proxy, sandbox, askpass, request-id and agent-store variables are excluded from the snapshot [R123, R141]. [SRC, quality 5]
- **Output discipline**: first 1 MiB streamed in-band, then `stdout_trimmed`; with `file_output_threshold_bytes` the full output spills to `agent-tools/<uuid>.txt` up to ~49 MiB; results carry `output_head`, `output_tail`, `elided_chars` and an `OutputLocation`; a rate limiter suppresses streaming for runaway output while the command keeps running [R122]. [SRC]
- **Background shells** write `terminals/<id>.txt` with a header (pid, cwd, command, status, running_for_ms refreshed every 5 s) and footer; output-pattern notifications can wake the agent [R015, R016]. [SRC]
- **Sudo** prompts are rewritten to `sudo -A` and routed to a host-supplied askpass socket with a secret [R124]. [SRC]

### 3.5 Read, images and screenshots [R127, R128]

The Read tool does a permission precheck (and can wait on a pending human decision), special-cases terminal and tool-output files, passes images and PDFs as data after a prepare-for-model step, streams large text with offset and limit and an 8 MiB cap, and runs every sub-step under a named timeout inside a trace span. Image sizes are read from PNG and JPEG headers, images are capped at 1024 px (1280 for the WebP screenshot path), two canonical computer-use sizes pass through untouched, and WebP output is shrunk by 20 percent per round until it fits 1 MiB. [SRC]

## 4. Permission, sandbox and governance model (category 10 and part of 7)

| Layer | Mechanism | Records |
|---|---|---|
| Per-machine policy | never / ask / always per action class, clipped by a team ceiling (`never` < `ask` < `always`), server-authoritative mode | R009 |
| Standing grants | scoped to an action or a recipient, per account scope, with retirements recorded in an approvals file | R010 |
| Approval copy | refusals and approvals are phrased for the model | R011 |
| Auto-review | up to 20 natural-language allow and block rules plus a risk classifier (`SmartModeClassifier`) | R012 |
| Prechecks | tree-sitter-bash AST (executables, redirects, command substitution) and allowlist prechecks for shell, MCP and web fetch | R064 |
| Hooks | ten wire events; PreToolUse can allow, deny or ask and rewrite input; Stop can inject a follow-up; names map to Claude hooks | R039 |
| Permissions service | pluggable; typed block reasons (needsApproval, userRejected, cursorIgnore, permissionsConfig, cursorFiles, adminBlock, adminCommandDenylist, unsafeResolution) | R125 |
| Native sandbox | policy readonly / readwrite / none, network allow and deny with strict mode, read boundary, capture of denied operations, Linux cgroup and OOM-score handling | R059-R062 |
| Egress | tunnel from the Box through the user's computer, tighten-only policy refresh, single holder | R021, R065 |
| Secrets | vault with domain-bound rules, 1Password, request-scoped env unset after each command, serialised push to Box, redaction before telemetry | R024, R025, R141, R146 |
| Governance | break-glass and bulk admin control, ZDR blocks the persistent computer, admin-disabled skills fail closed | R072, R102, R120 |

Notable decisions:

- **Fail closed where it matters.** If the list of admin-disabled managed skills cannot be read, all managed skills are dropped for that call [R120]; privacy mode blocks Box access outright [R102]. [SRC]
- **Tighten-only refresh** for egress policy: a loosening refresh is applied only on reconnect [R021]. [SRC]
- **Approvals travel with the request.** The server sends `skip_approval`, `smart_mode_approval`, `hook_approval_requirement`, an `admin_command_denylist` and a `secret_scope_id` on the shell call itself [R113]. [SRC]
- **Windows gap [R061]**: the Windows sandbox helper is a network proxy, not filesystem isolation, so `isSandboxHelperSupported` is false on win32; macOS has the full sandbox and desktop control, Linux (Box) has preflight. This is the build the user analysed. [SRC]

## 5. Agents with their own PC and browser (category 7, the techniques)

What the "agent PC" really is [R005-R008, R017-R023, R111, R129]:

1. **A cloud Linux desktop per user (Box)**, with run states ABSENT, HIBERNATED, RUNNING, STARTING, scheduled upgrades, migration phases and snapshot/restore of its object store with fenced manifest commits [R005, R053]. Clients send *intent* signals so work only wakes a sleeping computer when it matters (intent classified calls, pre-thaw, presence parking [R006, R007]). [SRC]
2. **Pixel-level control**: ComputerUseArgs actions (move, click, drag, scroll, type, key, wait, screenshot, cursor position) with settle time, a desktop lease actor id, a native X11 driver with xdotool fallback, a scaler from model frame to real pixels with an aspect guard, per-action recorder [R018]. The same protocol drives the user's own Mac through a native sidecar that is started only for input batches and always released [R017]. [SRC]
3. **No DOM automation library** is bundled; the "browser" is an ordinary browser on the Box desktop driven by screenshots plus secure-fill tools [R111]. [INF, medium: based on absence of Playwright or Puppeteer in the client and on the exec protocol]
4. **Human can watch and take over**: noVNC view, multi-screen switcher, takeover and hand-back [R020, R034]. [SRC]
5. **Identity portability**: import real Chrome cookies into the bot browser with per-origin approval [R022]; proxy WebAuthn to the user's own security key through a native signer [R023]; spend with virtual cards approved by passkey [R026]; egress through the user's IP [R021]. [SRC]
6. **Recording as an artifact**: ffmpeg x11grab proxy plus an input-event log re-rendered by a native "polished" renderer into a branded mp4 [R129]. [SRC]
7. **Second machine model**: the user's computer (and phone) register as machines; tools take a `machineId`, calls without one are refused with an instruction to call ListMachines when more than one exists; CopyToBox and CopyFromBox move files [R008, R036]. [SRC]

Limits: macOS-only desktop control in the local sidecar [R017], no Windows sandbox, one shared active desktop (lease semantics inferred) [R018], pixel-only browsing is slower and costlier than DOM actions, and everything depends on a hosted Box (cost, ZDR conflict [R102]).

## 6. Context, memory and skills (category 7 and 8)

- **Client-assembled request context** with per-section completeness flags and a deadline, large payloads by blob reference [R112, R057]; an **environment manifest** (OS, shell, time zone, folders, sandbox flags, mounted agent stores, computer-use support) is part of it [R130]. [SRC]
- **Filesystem-as-context**: transcripts for citation, notes, terminals, large tool outputs, plans, subagent state, plugins and skills all live under one layout with a managed ignore block [R121]. MCP servers appear as a file tree with `STATUS.md`, `INSTRUCTIONS.md`, per-tool JSON and a virtual `mcp_auth` tool; a meta-tool mode is gated [R115]. [SRC]
- **Rules, skills and custom subagents** are scoped files with environment enablement, globs and precedence tiers (Builtin, UserStore, UserHome, Workspace, Plugin); skills are discovered from `.cursor`, `.claude`, `.codex`, `.grok`, `.agents` [R120]; plugins follow a Claude-compatible manifest with SHA-pinned sources and secret variables [R119]. [SRC]
- **Memory**: facts (id, text, learned time), shards by scope with versions, a generated team summary with a memory version and stale flag, promotion from person to team [R030, R132]. Summarization is speculative at ~70 percent context usage with a PreCompact hook and a `ConversationSummary` boundary object [R100]. The algorithms are server side. [SRC for contracts, INF for behaviour]
- **Draft-first outbound messaging, approvals addressed to a person, agent-proposed rule changes** (product facet) complete the human-in-the-loop story [R033].

## 7. State, sync and transport (category 9)

- **Transcript watch**: one multiplexed server stream with per-agent cursors and generations; frame types connected, rows, cleared, cursor_too_old, agent_state, computer_actions, roster_changed, turn_failed, box_state, page_title_changed, heartbeat; bodies fetched separately; modes shadow-compare, store-first, server-tail for a safe migration [R044]. [SRC]
- **Send pipeline**: optimistic send, journal, nonce, echo confirmation, idempotent retry [R045]; the daemon also dedupes iMessage sends per key [R037]. [SRC]
- **Local replica** of roster, transcripts, drafts, layout for instant boot, a sync boot snapshot and a boot gate [R048, R077]. [SRC]
- **Agent store sync**: versioned files with etags, locks with TTL, tombstones, conflict copies, presigned multipart over 64 MiB, circuit breaker on token minting, resume detection by monotonic clock gap [R054]; a conflict journal with epoch and sequence cursor is exposed to the agent through `agent_store_conflict` [R001]. [SRC]
- **Resource states** (loading, ready, failed with previous, empty, unavailable) and write-supersedes-read guards in every cached resource [R134]; **named policies** for each wait (about 125 across bundles) [R049]. [SRC]
- **E2EE**: none observed. Encryption is at rest on the client (OS keystore, AES-256-GCM sealed daemon state [R066, R067]) and in transit (TLS). [SRC]
- **Scalability** is mostly server concerns; client side shows batching, backpressure limiters, stall watchdogs and server-tuned rates (about 260 dynamic configs [R055]). [SRC]

## 8. Telemetry, experiments, IPC, update, login (category 11 and part of 10)

- **Flags and experiments**: bootstrapped through `AnalyticsService/BootstrapStatsig` with a privacy header, disk-cached, jittered refresh (5 minutes production, 30 s dev), 84 renderer-visible gates, session-latched gates, local overrides [R135]. [SRC]
- **Analytics**: disabled when opted out; otherwise a deferred buffer of at most 256 events until activation; flush gated on token availability [R136]. **Logs**: batched structured log shipping with a 1000-entry buffer and explicit drop accounting by cause [R144]. **Crashes**: Sentry with process and phase tags; feedback attaches up to 5 Sentry event ids [R137]. **Tracing**: W3C trace context from keystroke to gateway command, OTLP export with retry [R052]. **SLO events**: 114 `sand.*` names [R147]. [SRC]
- **IPC**: tiny bridge, one capability-checked coordinator port [R143, R046]; sender validation and webview guest policy hardening [R069]. [REA, SRC]
- **Update**: staged installs, recovery, per-feature minimum client versions, update-while-away [R078, R079]. **Supervision**: coordinator relaunch with backoff and exit-class telemetry, renderer crash-loop guard [R145]. **Login**: browser device login with challenge and polling, API-key exchange, and a dev-login guard that refuses non-dev backends [R138]. [SRC]

## 9. What is best in class

1. Typed tool result unions with named failures plus uniform cancel, trace and machine addressing [R126, R001].
2. Stateful shell over stateless processes with secret hygiene [R123, R141].
3. Presence-aware wake and park of the agent computer, with intent classification so background work never wakes it [R006, R007].
4. Identity portability for an agent: cookie import with per-origin approval, passkey proxy, egress through the user's network [R021-R023].
5. Permission stack depth (ceilings, grants, auto-review, hooks, typed reasons, AST precheck) [R009-R013, R039, R125].
6. MCP-as-files and filesystem-as-context for token-efficient discovery [R115, R121].
7. Resumable multiplexed transcript sync with a safe migration mode ladder [R044].
8. Perceived-quality telemetry and bounded, self-auditing log shipping [R144, R147].
9. Polished recording of agent work as an artifact [R129].
10. Fail-closed governance defaults and ZDR handling by refusing the persistent computer [R102, R120].

## 10. Weaknesses and limitations

- **Windows is second class.** No filesystem sandbox, no local desktop control, no Messages, so "personal agent on my PC" is really "cloud Box plus a shell on my PC" here [R061, R017]. [SRC]
- **Pixel-only browser** [R111]: no DOM grants, no structured page model, higher cost; this is an [INF] from the absence of automation libraries and may change server side.
- **Dependence on a hosted computer** (cost, wake latency, ZDR conflict) and on the server for the actual agent loop; offline use is limited. [SRC for client dependence]
- **Mega-service**: 309 methods in one Connect service and a 117-name tool enum are hard to evolve; several features exist as wire messages only (human-change diffs, memory shards, notes folders) with no visible client behaviour. [SRC]
- **Typing lost on one hop**: user-computer frames carry JSON strings [INF from message shape].
- **Heavy gating**: 84 renderer gates plus ~260 dynamic configs make behaviour hard to reason about and put security-relevant defaults in remote config [R055, R135].
- **Agent self-administration** (CreateAgent, UpdateAgent, SetPrimaryBot, PublishToTeam, ConvertToTeam, rule proposals) concentrates a lot of authority in model-driven tools; the safeguards (approvals, drafts) exist but the surface is large [R003, R097].
- **No E2EE** and several privacy-relevant paths (cookie import, transcript tail, admin break-glass) rely on policy, not cryptography [R072, R022].
- **Unverified here**: server-side agent loop, memory algorithms, Box image contents, which hooks users can really configure, the polished renderer's output quality, the askpass provider.

## 11. What Verevon should copy, adapt or avoid

Grounding (from the Verevon baseline files and CLAUDE.md): Model Plane owns sessions/runs, sandboxes, browser grants, cost; **Quarry-v2 executes or rejects browser actions the Model proposes**; Zero Data Retention must propagate; Space access never widens an owner-plane ACL; personal agents are impossible today (personal Spaces cannot hold agents); a granted approval does not resume the paused run on the current path and there is one approval scope; the Space sandbox is a credential-free, no-network `code_interpreter` lease with a snapshotted workspace; there is no Space memory scope; "decisions that authorize future agent behavior come from outside the agent".

| Technique | Grok Bot mechanism | Verevon landing place | Constraint check | Effort |
|---|---|---|---|---|
| Typed tool result envelope with named failures | R126 | Model Plane tool contract; Space tool-call cards and audit | none | M |
| Closed enum of block reasons | R125 | Approval and Activity feeds; policy denials vs approvals vs sandbox denials | none; reinforces ACL layering | S |
| Approval scopes (once, conversation, always) and standing grants scoped to action or recipient | R009, R010 | Model Plane approval scope (audit lists it as blocked) | grants are issued by a human via Control, never by the agent | M-L |
| Approval continuation (resume the paused run after a decision) | approvals carried on the exec request, retire-approval frames | Model Plane run resume | closes the "granted but not resumed" gap | M |
| Hooks (pre/post tool use, stop, compaction) | R039 | Model Plane policy hooks owned by org admins | hooks run on org-controlled code, never inside the agent's authority | M |
| MCP-as-files and a needs-auth tool | R115, R116 | Sandbox workspace for Space agents; Space MCP cockpit | credentials stay in the broker, not in files | M |
| Filesystem-as-context (transcripts, notes, tool output, plans) | R121, R122 | Sandbox lease workspace hydrated at run start | ZDR: files must be purged or never persisted for ZDR orgs; ACL re-check on hydration | M |
| Stateful shell by snapshot-replay and per-call secrets | R123, R141 | Sandbox `process` service | no credentials in snapshots (already credential-free today) | M |
| Shell output spill with head/tail/elision | R122 | Process registry and Work tab | none | S |
| Environment manifest per turn | R130 | Run start payload and a human-visible "what can this agent do here" panel | none | S |
| Agent definition and memory shards | R131, R132 | Space agent export, audit, and a Space memory scope (Model vs Data ownership decision) | ownership decision pending; ZDR | M-L |
| Stale-while-revalidate plus supersede resources | R134 | Verevon v3 Space lists, presence, run status | none | S-M |
| Consent-gated, bounded telemetry and log shipping | R136, R144 | Verevon v3 and gateway | ZDR must propagate | S |
| Flags through the platform, latched per session | R135 | Gateway-served flags, no vendor SDK in the client | ZDR | S |
| Polished recording / replay of agent work | R129 | Quarry browser runs and sandbox sessions as a Space artifact | ZDR and privacy of recorded pages; Quarry owns capture | M-L |
| Process supervision and named policies | R145, R049 | Any Verevon desktop or sandbox host | none | S |
| Plugin and skill manifests compatible with Claude, SHA-pinned | R119, R120 | Space skills (the `/` picker already sends skill ids), Space MCP/plugin cockpit | supply chain: pin by commit, admin allowlist, fail closed | M |

**Agents-with-own-PC, a Verevon-shaped version** [INF]: keep Quarry as the only executor of browser actions and extend it with (a) a Space-scoped browser session carrying a Space grant (the baseline lists browser runs as not Space-scoped), (b) a streamed live view and takeover for room members who hold the grant, (c) Grok Bot's pixel path as a *fallback* when DOM actions are denied or impossible, still proposed by the Model and executed or rejected by Quarry, (d) recording as an artifact, and (e) an agent-owned persistent computer only as an opt-in tier for non-ZDR orgs, because Grok Bot itself blocks it under privacy mode [R102].

**Avoid or redesign**: agent-driven cookie import and login portability (Space membership must never widen an owner-plane ACL; use broker-held OAuth with per-origin human approval records instead), agent self-administration tools (turn into *proposals* that humans or Control decide, matching "decisions come from outside the agent"), a single mega-service, security defaults in remote config, and pixel-only browsing as the only browser path.

## 12. Mapping to the audit taxonomy

Record counts by category in `grokbot.agent-runtime.features.json`: Agents in the room 62, Governance security and compliance 25, Realtime and sync architecture 21, Client shell 14, Apps and extensibility 14, with a handful in Messaging core, Org and membership, Calls and meetings, Notifications, Search, Collaborative artifacts and UX (this facet is runtime, so those are intentionally thin and covered by the agent-product facet). Category 7 sits mostly in sections 3, 5 and 6, category 9 in section 7, category 10 in sections 4 and 8, category 11 in section 8.

Table stakes vs differentiators for the final hybrid (runtime scope): table stakes are typed tool calls with approvals, background work with progress, resumable transcripts and an idempotent send path; differentiators are the agent PC and its identity portability, presence-aware wake, MCP-as-files, stateful shell and the permission stack; nice-to-haves are polished recordings, perceived-quality telemetry catalogues and plugin ecosystem compatibility.

## 13. Open questions

1. How does the server decide between the Box and Temporal harness per message, and what does the harness migration do to history? (rollout statuses are visible, logic is not) [R004]
2. What exact browser runs on the Box and how are DOM-level needs met (cookie import implies a Chromium profile)? [R111, R022]
3. Which hooks can a Grok Bot user or admin configure in practice (the loader is not in the client)? [R039]
4. What do `symbolic_path_arguments`, `McpRecovery` outcomes and `PrecomputedHumanChange` do server side? [R117, R130]
5. How are request-scoped secrets resolved from `secret_scope_id`, and is there per-run revocation? [R141]
6. How is the Windows sandbox gap closed (Job Objects, WSL, containers), or is Windows meant to rely on the Box only? [R061]
7. What are the real limits of the desktop lease between a human viewer and the agent? [R018]
8. How does the server enforce ZDR for Box state, snapshots and admin break-glass transcripts beyond blocking access under privacy mode? [R102, R072]
