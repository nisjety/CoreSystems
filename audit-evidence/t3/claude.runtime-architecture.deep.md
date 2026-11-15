# Claude desktop 2.31226.1: runtime and architecture deep dive (Audit 3, facet "runtime-architecture")

Source: Claude desktop app 2.31226.1 (Anthropic PBC, package `@ant/desktop`), Windows x64 MSIX install, extracted `app.asar` copy plus the program files next to it. Companion data: `claude.runtime-architecture.features.json` (141 records, ids `claude-runtime-architecture-001` to `-141`). In this document `#NNN` means the record with that suffix. The scout inventory (`claude.inventory.md`) was read first and re-verified; section 13 lists what was corrected.

Goal of the facet: understand the runtime well enough to re-create it in Rust, and to say what Verevon (Model Plane plus the chat and agent pages) should copy, change or avoid. Categories mapped most heavily: 4 Agent runtime, 6 Computer and environment control, 13 Platform architecture.

---

## 0. Method, provenance and how to read the evidence

**Tags.** SRC = read directly in bundle source or program files. REA = observed through an REA tool result. DOC = Anthropic public documentation I actually fetched (`code.claude.com/docs/en/desktop`, 2026-10-13; the MCPB manifest page returned HTTP 503 and was not read). INF = my inference, with the basis stated. Nothing was launched, installed or modified; no per-user data directory (`AppData/Roaming/Claude`, `AppData/Local/Claude*`) was read, and no secret value was read or recorded.

**Citation convention.** The shipped JavaScript is minified. For line-addressable evidence I produced token-broken copies (a token-based line breaker built on the `@babel/parser` already installed with the REA CLI; it only inserts newlines at `{`, `}` and top-level `;`, so strings, regexes and templates are untouched). Evidence of the form `claude:.vite/build/<file>~L<n>` means line `n` of the copy at `scratchpad/tmp/claude-rt/pretty/<same path>`. The core chunk `index.chunk-EzXl5k4A.js` also exists there as `core.js`. Other evidence forms: `claude:install ...` (file listing or version file in the install folder), `claude:resources/...` (byte-level strings scan of a helper executable), `REA ev_...` (REA evidence id), `DOC ...`.

**REA usage log (tools that really ran in this pass).**
- `analyze_javascript_application` on a targeted subset directory (3.0 MB: `index.pre.js`, `index.js`, all preloads, the MCP hosts and the six workers; the 6.9 MB core chunk was left out on purpose because the scout's whole-app run exhausted the semantic budget). Result `ev_015aa68449005ca9009a2d0aa92c00b3fc9a98b0ed9cfb6e74c7fb33541c9cd0`, summary view `ev_ad009afc4495e7b69f7e22b6972460adddac00cd2353e0dffe449d0da4bea7c8`. Counters: 25 files, 24 parsed, 800,859 AST nodes, 52 contextBridge call sites, 1,504 IPC operations of which only 9 have literal channel names (1,495 dynamic), 1,278 renderer transmissions, 226 listeners, 0 main handlers resolved. Coverage partial: the semantic graph ran out of budget for `index.pre.js`, `mainView.js`, `mainWindow.js`, `directMcpHost.js`, `fileIndexWorker.js` and several others (23 limitations recorded, 14 application-graph unknowns).
- `inspect_analysis_view`: summary view and a modules page (30 of 219 entries; all hashed asset names, little semantic content), page-view evidence id starts `ev_bc4aa008e6b7f09e` (full id not retained).
- `trace_application_feature`: one literal string seed (`self-exec-blocked`, present in `nodeHost.js` by direct grep): no match (`ev_d7ee42a910babcd52ef901354281c82cbc81e9c225a8cf903857fecb5c627874`). This repeats the scout's finding: REA literal seeds do not reach the dynamically built eipc and message names.
- `inspect_pe_resources` on `resources/cowork-svc.exe` (`ev_f3c5370b120aaa7c8c8446818f8b81f6157f1a0bb8d5174bbbf507221869789c`) and `resources/chrome-native-host.exe` (`ev_a494703fc87abf04e7038f64098acde25b3ddc38a5029dd83525be74f50181eb`): both PE32+ x64, no resource entries. The language fingerprints (Go 1.25.14 and Rust) come from my own read-only byte scan of the same files, not from REA.
- Not used on purpose: `open_binary`/`binary_session` (shared active-target state), `search_strings`/`list_strings` (need an open target).
- REA's practical value here was Electron-level counters and honest limits; nearly all findings come from SRC reading.

---

## 1. One-paragraph model

Claude desktop is a thin, hardened Electron shell around three things: (1) the claude.ai web app loaded into a child view, which reaches the machine through a large typed IPC surface (89 services, runtime-validated, origin-gated; #096, #099, #100, #103); (2) a local agent runtime in which the **Claude Code CLI is the engine**, downloaded and updated by the app from signed manifests, driven through the Agent SDK with a pluggable spawn function so the same engine can run on the host, inside a VM or over SSH (#005, #006, #114); and (3) a "device" layer that gives the agent hands: a Cowork VM controlled by a privileged Windows service (#037 to #043), desktop computer use with tiered app access (#044 to #048), a Chrome extension bridge (#051) and a remote-tools bridge so cloud sessions can use the machine (#065). Around that sits an unusually mature governance layer: a schema-first managed-settings registry with fail-closed values and egress declarations (#117, #081), per-key agent-access classification (#116), signed catalogs and manifests (#114, #069), and, unreleased, CEL tool-policy rules with an external decision endpoint (#086). The native layer is already Rust (NAPI-RS, #125) plus Go and Rust helper executables (#128), which makes the stack a realistic template for a Rust re-creation.

---

## 2. Architecture map

```
                         claude.ai (remote)  or  app://localhost (bundled "ion-dist" UI + stub backend, 3P mode #122)
                                  |                                   ^
                  WebContentsView (preload mainView.js, sandboxed)     |  CSP per response, API shim
                                  |  window["claude.web"|"claude.settings"|...]   (#102)
                      eipc: typed, validated, origin-gated IPC (#099, #100) + MessagePorts (#105)
                                  |
  +-------------------------------+---------------------------- Electron main process (core chunk, ~178k lines pretty) -----------+
  | window/tray/menus/deep links (#091 #110)   managed config + flags (#117 #118 #119)   updater (#113)   telemetry (#120 #121)  |
  | LocalSessions / LocalAgentModeSessions (321 + 115 methods)   scheduled tasks   remote-control serving (#023)   device bridge (#065)|
  +--+-------------+-------------+---------------+----------------+-------------------+--------------------+----------------------+
     |             |             |               |                |                   |                    |
  utilityProcess utilityProcess  workers      NAPI-RS addon     Agent SDK chunk     VM client (RPC)     Chrome host / SSH helpers
  directMcpHost  nodeHost        pty, search, (registry, input, (lazy, 0.3.295)     named pipe, framed   (Rust, Go; peer-verified
  (#026)         (#027)          index, heavy  capture, TPM,     spawnClaudeCode-     JSON (#040)          pipes #126)
                                 (#106)        openBeneath #125) Process hook (#006)
                                                      |                |                    |
                                              Windows/macOS/Linux     Claude Code CLI      Cowork VM (Hyper-V / Virtualization / KVM)
                                              APIs                    (Bun binary,         guest agent, gVisor net, mounts (#037-#041)
                                                                       signed manifest #114)  runs shell; file tools on host by default (#024)
```

Process inventory (SRC): main process; one BrowserWindow for chrome plus one child WebContentsView for the web UI (#096); popouts adopted from `window.open`; artifact, preview and consent windows (#031, #052, #061); two MCP utility processes (#026, #027); seven workers (#106); the CLI child (#005); VM service and guest (#039); Chrome native host (#051); SSH helpers (#014).

---

## 3. Process model and IPC

### 3.1 Shell and windows
- Entry `index.pre.js` installs a V8 code-cache hook (precompiled `.jsc` per file and arch) and Sentry bootstrap, then loads the chunk graph; the SDK (`D6Q00DJz`) and the Remote Control bridge (`DcP6Ha83`) are lazy-loaded with timing logs (#108). ~330 hashed chunks come from Vite/Rolldown code splitting. Version facts: Electron 44.4.3 (install `version` file), exe 246 MB, Forge 8 alpha, MSIX plus Squirrel (#096, #112).
- Every window uses one frozen webPreferences baseline (sandbox, context isolation, no node integration, no webview tag; devtools only when not packaged) and untrusted previews additionally run `--js-flags=--jitless` (#097, #052).
- Thirteen preloads, one per window role, expose only what that role needs (#098). The main web view's preload builds namespaced objects (`window["claude.web"].LocalSessions...`) per origin (#102) and receives boot data through `additionalArguments` (#104).
- Custom privileged schemes (`cowork-artifact`, `cowork-file`, `claude-media`, `claude-simulator`, `app`) bind each content kind to its own session partition; file-preview and artifact sessions deny every permission (#109).

### 3.2 eipc
- `ao(prefix, name, senderValidator, {methods, stores, events})` registers `ipcMain.handle` channels named `$eipc_message$_<uuid>_$_<namespace>_$_<Service>_$_<method>`. Methods are `[name, [[arg, validator]...], resultValidator, 'sync'?]`; both arguments and results are validated; stores add `getState`, `getStateSync` and a push channel; events push validated payloads (#099, #101). `@ant/ipc-codegen` generates the client.
- Defence in depth: the sender must be the top frame of an allow-listed origin both in the preload (before the object is exposed) and in main; development origins need an override flag (#100).
- Scale: 89 services in 13 namespaces; the two session services have 321 and 115 methods (#103). I treat that as a smell (god object), not as a model.
- REA could see only 9 of 1,504 IPC operations by literal name, so any Rust port must work from the service tables (scout `eipc_services_v3.txt`) rather than from static IPC analysis.

### 3.3 Workers and supervision
- A reusable worker client forks a utility process, passes one MessagePort in an init message, correlates requests by id, uses the first reply as proof of health and applies cooldown backoff if the worker dies or hangs with requests in flight (#131). Workers: PTY host (#053), shell-PATH resolver (#133), transcript search (#022), file index, heavy work (git plumbing), MSAL broker (#034), stall sampler (#107).
- Stall sampler: shared `BigInt64Array` heartbeat, an Atomics poller and an inspector `Profiler` session that records only while the main thread is stalled, rate-limited by a token bucket (#107). This is the cleanest diagnostic idea in the app and ports directly to Rust.

---

## 4. The local agent runtime

### 4.1 Engine, SDK and distribution (#005, #006, #114)
- The agent loop is **not** in the desktop code. The desktop launches the Claude Code binary (a Bun-compiled executable; error classifiers match "Bun has crashed") and talks to it through the Agent SDK (0.3.295, lazy chunk). SDK options set by the host: `pathToClaudeCodeExecutable`, `executableArgs`, `cwd`, `model`, `effort`, `maxTurns`, tool lists, hooks, `canUseTool`, `getOAuthToken`, MCP servers (`type:"sdk"` in-process servers plus stdio/HTTP), and `spawnClaudeCodeProcess`.
- Distribution is a small supply-chain system: a catalog embedded in the app (version 2.1.295, per-platform checksums, `sdkCompat.testedWrapperVersions`, `harnessSchema`) and an hourly poller (jittered, check every N ticks, default 4) that reads `/stable`, fetches a manifest plus a `.raw-sig.json` sidecar, verifies RSA-SHA512 over a domain-separated message with an embedded key, checks version floor and wrapper compatibility, optionally spreads the rollout over N hours using `sha256(installId, version) mod window`, and swaps the binary. It is flag-gated, skipped when flags are stale for two hours, disabled for MSIX and by enterprise policy. `manifestSignatureEnforcement` is `"flag"` in this build, i.e. not hard-enforced (#114).
- Credentials: tokens reach the engine by callback (`getOAuthToken`) or spawn parameter, never by plain env; a per-session secrets directory plus an explicit credential-variable scrub list handle third-party providers (#124, #007).

### 4.2 Spawn seam (#006)
`spawnClaudeCodeProcess` is the seam that makes placement a runtime choice. For VMs `createVMSpawnFunction` returns a process-like object whose stdin, stdout, stderr and exit are bridged through the VM client; for SSH the backend tunnels to a remote helper and supports reattach. Host code (permissions, mounts, consent UI) does not change.

### 4.3 Execution modes (#024, #085)
Default is **host-native**: the agent loop and file tools run on the user's computer with path-based access control, shell commands run in the VM. `requireCoworkFullVmSandbox` moves the loop into the VM (DOC and SRC agree). In host-native mode the safety boundary for files is a PreToolUse hook that: rejects root- or drive-relative paths and Windows alternate spellings (streams, 8.3 names, device prefixes, trailing dot/space); resolves symlinks safely; carves protected locations (system, credentials, Claude-internal data) out of granted folders; refuses searches whose root contains a protected location; fails closed with an explanatory message (#085). The credential-path classifier behind it is the best of its kind I have seen (#056).

### 4.4 Permissions and consent (#008, #089, #057)
- `canUseTool` first rejects tool names that mimic internal card names, then applies surface gates, then escalation (attended vs unattended, dispatch children, kill switch). Denials carry model-directed text ("retry in an attended session", "steer to request_access").
- Self-management tools (rename session, start side session, bind PR) use nonce-bound, single-use consent via PreToolUse output with `updatedInput`, deny hidden-text titles and block subagent self-rename (#089).
- Privileged native actions requested by web content must follow a real user gesture within about 1.5 s (#057).

### 4.5 Sessions, delivery and recovery (#010, #011, #012, #013, #015)
- One session table spans chat, Cowork, Code and imported CLI sessions with typed origins and a trust gate on imports (#010).
- Messages have uuids; pending cycles, deferred sends, held steers and "written but unacked" sets let the host rebuild state after a reattach gap without duplicates or loss (#011).
- SSH sessions survive link loss: remote process identity, sequence-numbered replay, orphan reaping with kill escalation, a process-loss brake and limiter, and explicit "turn was cut" handling (#012). A versioned remote helper is upgraded only when sessions are quiet, with an org-required launcher hook (#013). Two SSH transports (ssh2 or system OpenSSH) and a Go askpass/broker avoid typing secrets into agent-visible terminals (#014).
- Failure taxonomy: exit codes (including Windows fast-fail and DLL-not-found), Bun crashes, stream stalls, sleep interruptions, gateway sign-in problems, strict-sandbox 400s (#015, #016).

### 4.6 Background work (#017, #018, #019, #021, #023)
Keep-awake claims through `powerSaveBlocker`; a "stay reachable" mode with an OS wake helper that backs off on battery; scheduled tasks as local files with approval lifetimes; dispatch sessions started from a phone are unattended and get short-lived computer-use grants; local folders are registered as remote environments with registration states; the VM is kept warm for an idle grace period between turns; a local Code session can be teleported to a cloud environment after a readiness check and a transfer plan (#139).

---

## 5. The Cowork VM and the Windows service

This is the most re-usable subsystem for a Verevon sandbox.

- **Backends behind one interface (#037).** Windows: Hyper-V (HCS) driven by `cowork-svc.exe` (Go 1.25.14; gvisor-tap-vsock, virtsock, miekg/dns, tcpproxy, gopacket, dhcp, win-httpproxy, go-winio; REA confirmed the PE, byte scan gave the packages). macOS: the Swift addon (Virtualization framework; logs name gVisor, INF). Linux: KVM with vhost-vsock (typed failure reasons). The TypeScript `vm` object has about 25 operations (#037).
- **Bundle (#038).** `vmBundleManifest` embedded in the app: four retained versions (2026-02-18, 2026-03-30, 2026-06-04, 2026-09-11), per-OS/arch files (`rootfs.img` raw 10 GiB, about 1.2 to 1.3 GB compressed, or `rootfs.vhdx` plus `vmlinuz`, `initrd`, `initrd-micro`), checksums of compressed and raw forms, progress ranges for one progress bar and delta files from five previous versions (170 to 520 MB) applied by a native `applyVmImageDelta` with zstd large-window decoding; a 37.7 MB `smol-bin.x64.vhdx` tools disk ships in the install folder.
- **Service lifecycle (#136).** The unprivileged app queries the Service Control Manager, starts the service when stopped (skipping while the MSIX package is being serviced), classifies outcomes (running, denied code 5, failed 1058/1060/1072, exited, timeout) and backs off 5 s to 5 min.
- **Pipe identity (#126).** The client connects through a native call that requires a SYSTEM-owned server pipe and caps impersonation; packaged builds refuse to fall back. The service verifies the client's package family name and exe path (Go symbols `getPackageFamilyName`, `pathIsUnder`, INF for behaviour).
- **RPC (#040).** 4-byte big-endian length plus JSON, max 10 MiB; `{method,id,params}` requests, `{id,success,result|error}` responses, a second connection for `subscribeEvents` (stdout, stderr, exit with oom and peak bytes, error, networkStatus, apiReachability, startupStep) and guest-initiated requests answered with `sendGuestResponse`. Idempotent calls retry once on a closed pipe.
- **Processes and mounts (#041).** `spawn({id,name,command,args,isResume,cwd,env,additionalMounts,allowedDomains,oneShot,mountSkeletonHome,oauthToken})`; later `mountPath(processId, subpath, mountName, mode, hide[])`; working dirs `/sessions/<id>`; layered log tags (vm, hcs, hvsock, vnet, hcn, vhdx, hidefs, tapvsock, skeletonfs).
- **Startup (#042, #063).** Steps `download_and_sdk_prepare`, `load_swift_api`, boot, guest-connect polling, `sdk_install`; typed failure causes (hypervisor virtualisation disabled, logon right denied, VHDX compressed or encrypted, VPN routing conflict, host share not mounted...); auto-reinstall at most once and never for download, service, share, user-request, hypervisor or disk-full failures; a heartbeat pings every 30 s and restarts after five failures.
- **Egress (#043, #064).** Allowed domains per process; the `*` wildcard is stripped under the HIPAA gate; guest proxy and PAC fetch; the Windows service injects host trusted roots; proxy log strings mention TLS interception and the token is handed over out of band (INF: credentials are likely injected at the proxy, never placed in the guest).

---

## 6. Computer use

- **Capability profile per OS (#044).** macOS 14+ (app-scoped 15+) with native window exclusion in screenshots; Windows "mask" mode (excluded window rectangles masked, fail-closed sub-gate); Linux/Wayland through portals with no filtering and no hit test. Sub-gates: hide-before-action, clipboard guard, auto target display, adaptive resolution, mouse animation, multi-line clipboard paste, pixel validation (off).
- **Tiered app access (#045; DOC agrees).** Browsers are view-only (use the Chrome tool), terminals and IDEs are click-only (use Bash), everything else is full; enforced at call time by the frontmost-app check, with a user-maintained denied-apps list.
- **Prompt-injection hardening (#046).** The installed-app list embedded in the tool description is NFC-normalised, character-allow-listed, length-capped and truncated; link-safety rules are part of the instructions (#080).
- **Machine lock and restoration (#047).** One session at a time (global lock and glow overlay); turn end unhides hidden apps and restores the clipboard; unattended turns are denied approvals with a steering message (#008).
- **Background mode (#048).** `app_*` tools act on a window without taking the foreground (macOS 15+); approvals for bringing windows forward are stricter. Windows support for this mode was not observed.
- **Native primitives (#125).** `cuListDisplays`, `cuListOpenWindows`, `cuAppUnderPoint`, `cuControlAtPoint`, `cuFocusedControl`, `cuHideApps/cuUnhideApps`, masked/excluded window rects, `typeText`/`typeTextPaced`, `moveMouse`, `mouseButton`, `mouseScroll`, `validateKeys`, `focusWindow`, `getFrontmostAppInfo`, `watchRecorder*`, `watchVoiceover*` (Rust).
- **Learn-from-user (#049) and live view (#050).** Teach mode and watch-and-record with voiceover capture; a framebuffer preview with pointer/key/scroll back-channel over a MessagePort.
- **Browser (#051).** Claude in Chrome: native-messaging manifest for six Chromium-family browsers, a Rust native host, three transports (same-user-verified pipe, cloud relay, hybrid) and org gating. In-app previews are permission-denied, optionally jitless (#052).
- **Terminal (#053).** PTY in a worker with a disk-backed, ANSI-state-aware scrollback log, streaming secret redaction, grep/window reads and a shell fallback.
- **Office (#138).** The app sideloads Claude's Office add-in by registering the WEF folder as a trusted catalog under HKCU (via the native registry writer), writing per-app manifests, opening the file and waiting for the bridge; COM focus and conversation handoff complete the loop.
- **Cloud sessions using the device (#065).** A "device" registers tool sets (computer, browser, file, direct MCP inventory, bash when allowed), publishes announcements and answers bridge RPC with per-kind deadlines.

---

## 7. MCP and extension hosting

- Config: `claude_desktop_config.json` with per-server schema validation and a visible skip report (#025). The Code tab merges it with `~/.claude.json` and `.mcp.json` (DOC).
- **Direct MCP host (#026).** A utility process is both an MCP client to a remote HTTP/SSE server (Electron `net.fetch`, so proxy and certificates work; timeouts; version-negotiation probe with fallback) and an MCP server on a MessagePort for the engine. 401s go back to main for refresh, elicitation is forwarded, session-expiry (404) closes the port so it re-parks, three terminal errors close it.
- **Node host (#027).** Node-based extensions run inside the app's own Node: stdin/stdout are replaced by a port, the entry module is imported with a faked argv, and `child_process` is patched so nothing can spawn the Electron binary as Node (redirect to a non-existent file, a named error). Python extensions use a pinned uv 0.9.7 with per-platform SHA-256 (#030).
- **Extensions (#028, #029).** MCPB/DXT manifest v0.1 to v0.4 (server types node/python/binary, user_config with `sensitive`, compatibility); install checks hash, manifest, signature info and source against allow and block lists, and installed extensions are re-checked and disabled when lists change; admins can require signatures (fail closed).
- **Artifacts (#031, #141).** Sandboxed HTML with an SRI-pinned CDN allow-list (Chart.js 4.5.0, Grid.js 5.0.2, Mermaid 11.15.0), per-artifact partitions, a webRequest filter, a CSP and a narrow host bridge for MCP tool calls.
- **EngineSurface (#032).** A pane/scroll/focus/toast protocol that carries Claude Code plugin UI into the desktop renderer (medium confidence on purpose).
- **Plugins and skills (#033).** Marketplaces with allow lists; six skills bundled with an always-on tier.

---

## 8. Configuration, policy, telemetry, update, persistence

- **Managed-settings registry (#117, #081, #087, #137).** One schema describes every enterprise key: registry name, legacy name and its effect, scopes (1p/3p), `availableInVersion`, executionTarget, readers (desktop, m365, code), title, default, `failClosedValue`, egress requirements as a function of the value, visibility, remote-policy metadata (hot-applied, hybrid scope), predicates for show/warn, deprecations with rewrites. The same registry drives validation and the admin UI. Policy shapes are versioned so an older client reads a newer policy safely or fails closed (for example MCP tool permissions: "ask-session" rewritten to "ask", unknown permission means blocked).
- **Tool-policy as code (#086, unreleased).** `toolPolicyRules`: CEL conditions (<= 4096 chars), actions allow / deny / require_approval, reasons required for non-allow, <= 100 rules, order-independent with deny wins, fail-closed stand-in rule; `toolPolicyEndpoint`: HTTPS policy service with loopback/query rejection, optional credential helper, `onFailure` default deny, timeout 1 to 10 s (default 5). Marked `@next`, off-device only (self-hosted sessions beta).
- **Tiers (#118).** HKLM/HKCU policy keys, MDM profile, root-owned file; unreadable managed tier disables local settings.
- **Preferences (#116).** Zod schema where each key carries "agent may not access" metadata with a reason; main-only keys (grants, trusted folders, preview allow-lists) cannot be set by the web page; some maps can only shrink from the renderer.
- **Flags (#119).** GrowthBook with remote, disk-cache and hard-coded fallback; numeric hashed flag ids; freshness requirements on risky features.
- **Telemetry (#120, #121, #075, #076).** Batched queue with jitter, kill switch, CI withholding, HIPAA rescrub; allow-list scrubbing (known names only, sentinel or salted hash otherwise); typed event vocabulary; process and VM memory digests; customer OTLP export including from inside the VM (#074); scrubbed support bundles and heap/memory traces (#073).
- **Update (#113).** First-party feed with stable/latest channel, idle-aware checks, delta payload bookkeeping, `dangerousMaxVersion` cap, restart-when-idle that counts running sessions.
- **Persistence (#115, #135, #140).** JSON config in userData, directory-per-concern layout (sessions, VM bundles, worktrees, secrets, caches), an account/org-scoped tree with Windows short-path compaction and dual-path resolution (#140), safeStorage for secrets, per-session secret directories, preview-before-delete storage cleanup and "reset VM/sessions" recovery actions.
- **Third-party mode (#122, #123).** Bundled `ion-dist` UI served from `app://localhost` with a per-response CSP and a stub backend (`/api/bootstrap`, system prompts, plugin lists, title generation through the configured provider, 503 JSON for the rest); a deployment profile object selects provider (Anthropic, gateway, Vertex, Bedrock, Mantle, Foundry), credential kind (static, helper-script, interactive, external IdP, vendor profile, workforce), models and managed MCP servers.

---

## 9. Cross-cutting security themes worth copying

1. Two-sided origin and top-frame checks for every native capability (#100).
2. Fail-closed defaults declared next to each setting, plus egress declarations (#081).
3. Peer-identity verification on local pipes in both directions (#126).
4. Capability-style file access and a credential-path classifier (#056, #085, #127).
5. Model-directed refusal messages that steer the agent to a safe alternative (#008, #085).
6. Nonce-bound, single-use consent for self-modification (#089).
7. User-gesture gating for native actions (#057).
8. Sanitised interpolation of untrusted names into tool descriptions (#046).
9. Hardened git posture (hooks off, fsmonitor off, private index, subcommand allow-list) (#054).
10. Allow-list telemetry and scrubbing (#121); fail-isolated observability callbacks (#082).
11. Signed catalogs and manifests with compatibility matrices and staged rollout (#114, #069).
12. Compliance gates as named, reason-coded switches (#077).

---

## 10. What is best-in-class here, and what is weak

**Best-in-class (relative to Codex, Cursor, T3 Code, Manus, Antigravity, Hermes and the DeepSeek harness; judged from structure, not from running them).**
- Engine distribution and compatibility management (#114).
- The VM/service/RPC design and Windows hardening (#037 to #043, #126, #136).
- Host-native vs full-VM split with a concrete file-confinement hook (#024, #085).
- Policy surface: registry, fail-closed values, versioned shapes, CEL and external PDP (#081, #086, #087, #117).
- Remote-process resilience for SSH (#012).
- The preference/agent-access classification (#116).

**Weak or risky.**
- Remote-UI coupling: the shell only works with the matching claude.ai build; `availableInVersion` gates and ~120 feature keys are the price (#103, #119). Offline use is impossible except in 3P mode (#122).
- God services (321 and 115 methods) and codename-heavy, hashed flags make the system hard to audit (#103, #119).
- Electron weight (246 MB executable, ~330 chunks, V8 cache hack to hide startup cost) (#108).
- Windows computer use is less capable than macOS (no app-scoped background mode observed) (#048).
- Signature enforcement for the engine manifest is "flag" mode, not hard (#114).
- No local audit log was found in this pass (negative evidence, not exhaustive); telemetry serves the vendor, not a user-held audit trail (gap).
- No eval or replay harness for the host (scout negative; not found in this pass either).
- SOCKS proxies unsupported for downstream agents (#132).
- Three layers on Windows (Electron, service, guest) mean many failure classes; the startup state machine and error taxonomy exist because of it (#042, #015).

---

## 11. Rust port design notes (explicit)

### 11.1 Mapping Electron features to Rust
| Claude (Electron) | Rust equivalent | Notes |
|---|---|---|
| Main process, windows, tray, menus, deep links, single instance | `tauri 2` (wry/tao) or a thin `tao`+`wry` shell; `tauri-plugin-single-instance`, `deep-link` | Keep the SolidJS UI as the web app. WebView2/WKWebView/WebKitGTK replace Chromium; no `WebContentsView` layering, so implement title bar and overlays as separate webviews or in-page. |
| Preload + contextBridge, role-specific | Tauri capabilities per window label plus an injected init script that exposes only the role's namespaces | Re-implement the origin and top-frame check in the init script and again in the command handler (#100). |
| eipc typed IPC | A `vd-ipc` crate: `#[service]` derive generating handler traits, TypeScript client (specta/ts-rs) and runtime validators (serde + validator or schemars JSON Schema) | Result validation too. Store and event channels via `tauri::ipc::Channel`. |
| MessagePort side channels | `tauri::ipc::Channel`, a loopback WebSocket, or custom protocol streaming | Use for transcripts, frames, terminal data. |
| utilityProcess workers | `tokio` tasks or child processes with the supervisor in #131 | Same protocol shape: init, request ids, health proof, cooldown. |
| NAPI-RS native addon | A `vd-native` crate used directly (no FFI boundary) | Export list in #125 is the requirements list. Windows: `windows` crate (registry, SCM, HCS, named-pipe peer SID/PID), `uiautomation`, SendInput/`enigo`, DXGI/`windows-capture`, NCrypt for device keys, `cap-std` or NtCreateFile-relative for `openBeneath`. macOS: `objc2` (CGEvent, ScreenCaptureKit, Virtualization). Linux: `ashpd` portals. |
| Agent SDK + Claude Code child | `vd-agent-host` with an `AgentProcess` trait (Local, Vm, Ssh, ModelPlaneLease) | For Verevon the "engine" can be the Model Plane execution loop reached through the gateway rather than a local CLI; keep the spawn-seam idea (#006) so local and remote runs share semantics. |
| CLI manager | `vd-engine-manager`: manifest fetch, `ring`/`rsa` verify, `sha2`, `zstd`, staged rollout hash, floor/compat checks | Make signature enforcement hard from day one and add key rotation lists. |
| VM client + Windows service | `vd-vm` trait + RPC; service in Go (keep gvisor-tap-vsock) or Rust (`smoltcp`, `hickory-dns`, `rustls`-based MITM) | Start with the same framing (u32 BE + JSON) for debuggability; version the schema; later move to protobuf. |
| DXT/MCP hosts | `rmcp` (official Rust MCP SDK) for client and server; supervisor for processes | Node extensions need a Node runtime: ship or download one; keep the self-exec guard idea (env flag plus PATH shim). |
| Managed-settings registry | `vd-policy`: serde structs with a derive that emits per-key metadata (scope, version, fail-closed value, egress); tiers (registry, plist, file, remote) | CEL via `cel-interpreter`; external PDP via `reqwest` with deny-by-default. |
| Telemetry, Sentry, OTLP | `sentry`, `opentelemetry` + `tracing`; allow-list scrub crate | Add a local append-only audit log with a hash chain (gap above). |
| Stall sampler | A watchdog thread plus `pprof-rs` or `tracing` spans | Heartbeat via `AtomicU64`. |
| PTY + scrollback | `portable-pty` + `vte` + segmented log with streaming redaction | Port #053 as a crate with fuzz tests for escape-sequence boundaries. |
| Git harness | Hardened `git` CLI invocation with the same config flags; `gix` for read-only queries | Subcommand/flag allow-list as data (#054). |
| Updater | `tauri-plugin-updater` or custom: signed JSON feed, idle-aware scheduling, MSIX App Installer on Windows | Keep restart-when-idle (#113). |

### 11.2 Proposed workspace layout
`vd-shell` (bin) -> `vd-ipc` -> `vd-session` (session table, delivery state machine #011) -> `vd-agent-host` (spawn seam) -> {`vd-vm`, `vd-ssh`, `vd-local`, `vd-lease`}; `vd-native` (OS); `vd-cu` (capability profile, tiers, lock, hide/unhide, app list sanitiser); `vd-mcp` (supervisor, direct host, stdio host, OAuth broker); `vd-policy` (registry, tiers, CEL, PDP client, fail-closed); `vd-sandbox` (path classifier, confinement hook, git posture, PTY recorder); `vd-engine-manager`; `vd-update`; `vd-telemetry` (queue, scrub, OTLP, audit log); `vd-chrome-host` (native messaging host); and a Go or Rust `vd-vmsvc` Windows service.

### 11.3 Wire contracts to define first
1. `vd-ipc` command/event schema (JSON Schema source of truth, generated Rust and TypeScript).
2. VM RPC: methods in #037 plus `subscribeEvents` and guest-initiated requests (#040).
3. Device bridge: register, announce tools, call with per-kind deadline, heartbeat with adaptive interval and delivery acks (#065, #129).
4. Policy document: versioned, with readers matrix, fail-closed values and egress declarations (#081, #087, #137).
5. Engine manifest: signed, compat matrix, rollout spread (#114).

### 11.4 Improvements over Claude desktop
- Hard signature enforcement, key rotation and a transparency log for engine and VM bundles.
- A local, tamper-evident audit log of every grant, approval and tool call, linked to the Control Plane audit stream.
- Resumable, sequence-numbered event streams for every run (they only have this for SSH, #012), shared by local, VM and remote runs.
- Splitting the 321-method service into session, delivery, git/PR, remote-host and approval services with separate capability sets.
- Windows-native background computer use (UI Automation plus per-window messages) to match macOS app-scoped mode (#048).
- First-class eval and replay harness for host decisions (permission, confinement, egress), fed by recorded sessions.
- Offline-capable UI by shipping the SolidJS app locally and serving the Verevon gateway contract through a local shim in the same way as `ion-dist` (#122), but as the default rather than the enterprise exception.
- Treat Zero-Data-Retention as a host-level mode: no local transcript persistence, scratch dirs wiped, support bundles scrubbed, telemetry rescrubbed (#120, #121).

### 11.5 Phased plan (suggested)
1. Shell + typed IPC + policy registry + telemetry skeleton (no agent).
2. `vd-agent-host` against the Model Plane gateway; permission/consent hooks (#008, #089, #085); transcript streaming (#001).
3. `vd-sandbox` (path classifier, confinement, PTY recorder, git posture) and local file tools.
4. `vd-vm` + Windows service + VM bundle manager with deltas.
5. `vd-cu` (macOS/Windows), Chrome host, device bridge.
6. SSH/remote runners with reattach; scheduled and dispatched work.

---

## 12. Mapping to Verevon planes and rules

- **Control Plane** owns identity, orgs, entitlements and audit: the managed-settings registry, device registration (#065, #129), signing keys and audit of grants belong there; the desktop only consumes signed policy (#117, #137).
- **Model Plane** owns sessions/runs, sandboxes, browser grants and cost. Claude's local analogues are LocalSessions (#010, #011), the VM and its leases (#037 to #043), computer-use grants (#045, #083) and the effort/model clamps (#070). The Verevon baseline already states that desktop computer use is parked and sandbox-manager is lease bookkeeping only; this audit gives the concrete contract list if that changes.
- **Rule "Model proposes, Quarry executes"**: Claude's equivalent is "engine proposes, host `canUseTool` decides, host or VM executes". For desktop computer use the Verevon equivalent is a device executor that executes or rejects Model Plane proposals with the same tier and lock policy (#045, #047).
- **No cross-plane DB access**: the desktop must treat the gateway as its only authority; local state is cache plus secrets (#115).
- **Zero Data Retention**: propagate into local transcript storage, scratch, bundles and telemetry (#121).
- **Space access never widens an owner-plane ACL**: folder grants are intersected with Space ACLs; grants are main-only records with consent provenance (#083, #116).

---

## 13. Corrections and additions relative to the scout inventory

- `index.chunk-D6Q00DJz.js` is the Agent SDK 0.3.295 wrapper (lazy-loaded), not an embedded CLI; the CLI is a separate downloaded Bun binary with a signed manifest (#005, #114).
- The Windows VM layer is a Go service (`cowork-svc.exe`, Go 1.25.14, gvisor-tap-vsock and related), not a .NET service; `chrome-native-host.exe` is Rust; SSH helpers are Go (#128).
- "Host loop" is verified: default host-native loop with file tools on the host, shell in the VM; full-VM mode is an admin switch (#024).
- Electron version is 44.4.3 (install `version` file); the scout's "not observed" referred to the asar only.
- New findings not in the inventory: CEL tool-policy and external PDP (#086), versioned policy schema (#087), nonce-bound consent (#089), device-as-tool-host (#065), system-wide writing assistant (#066), preference agent-access classification (#116), host-side egress proxy hints (#064), PTY flight recorder (#053), git posture (#054), credential-path classifier (#056), user-gesture gating (#057), stall profiler (#107), worker supervisor (#131), window manager library (#134).
- REA: literal seeds still cannot reach the dynamic channel names; the module trace is not useful for this app.

---

## 14. Open questions and blockers

1. On-disk transcript and session-record formats (engine-owned; not read; no user data directory was opened).
2. The VM guest agent and the exact credential-injection behaviour of the egress proxy (only host-side strings and RPC names seen).
3. The CEL environment (variables available to `condition`) and where policy is evaluated; likely engine or server (#086).
4. How Windows "mask" screenshot filtering is implemented in the native addon (#044).
5. EngineSurface's plugin side and the owner of the rendering protocol (#032).
6. The contents and diff policy of `ion-dist`, and how the UI decides between 3P-stub and live modes beyond the managed config (#122).
7. Whether Electron fuses are set (build-time only) and what the Chromium version is.
8. Which flags are on for real users; build defaults only ("unavailable" is not proof of absence).
9. Whether `sableOtter` (#066) and Windows app-scoped computer use (#048) ship to users.
10. Bridge wire payloads for Remote Control and the device bridge (#129, #065).

---

## 15. Record index by category (ids are `claude-runtime-architecture-NNN`)

- Chat experience: 001
- Multimodal and creation: 002, 003
- Research and web: 004
- Agent runtime: 005 to 024, 139
- Tools, plugins and extensibility: 025 to 036, 141
- Computer and environment control: 037 to 066, 138
- Coding capability: 067
- Memory, learning and personalization: 068
- Models and routing: 069 to 072
- Transparency, observability and control: 073 to 076
- Safety, permissions and governance: 077 to 089
- UI/UX and product design: 090 to 095
- Platform architecture: 096 to 137, 140

Quality is a 1 to 5 maturity rating relative to the other reference apps; high ratings reflect structure and execution visible in code, not user-visible polish. Evidence totals: 141 records, 922 evidence items; every `~L` line reference was checked mechanically to exist (0 failures) and a keyword check flagged only function-declaration lines whose minified names carry no keywords (reviewed).
