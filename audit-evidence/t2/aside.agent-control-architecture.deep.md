# Aside browser 1.0.928.2 - agent-control architecture (deep dive)

Facet: how the AI controls the browser and shares it with the user (AsideAgentManager, AsideDaemon, aside_proxy, IWA key distribution, MEIPreload, IPC and process model, security and permissions, how agent actions are surfaced). Maps to taxonomy categories 9 (AI-browser control) and 7 (observability, operations, security), and to what Verevon should copy or avoid.

Provenance tags: SRC = read directly in installed program files or in the daemon's embedded JS payload; REA = observed through an REA tool; DOC = documentation (none shipped); INF = my inference, basis stated. Confidence is stated per claim where it is not high.

Companion file: `aside.agent-control-architecture.features.json` (86 records, ids aside-agent-control-architecture-001 to 086).

## 0. How to read the evidence

- AM = `C:/Program Files/Aside/Application/1.0.928.2/AsideAgentManager` (MV3 "Aside Browsing Agent"; `background.js` is pretty-printed, 11,836 lines, so `background.js:LINE` cites are exact).
- PM = `.../AsidePasswordManager` (MV3 "Aside Vault").
- SK = `.../AsideDaemon/win-x64/static/skills/builtin` (32 SKILL.md packages).
- `daemon_sea_blob.bin@N` = byte offset N (latin1 index) in `scratchpad/tmp/aside/daemon_sea_blob.bin`, the scout's copy of the NODE_SEA_BLOB resource of `aside-daemon.exe`. The 11.79 MB ESM bundle starts at byte 92; I extracted it to `scratchpad/tmp/aside-daemon-src/index.mjs` (offset there = N - 92). The bundle is minified but keeps template-literal prompts, tool descriptions and the injected page script verbatim, which is why so much behaviour is readable.
- DLL = `Aside.dll` (Chromium 153 fork, 306 MB), scanned with `grep -a` for literals only; it was not opened in a disassembler.
- Not read, per the audit rules: any per-user data directory (Aside User Data, AppData profiles), cookies, tokens. One PostHog public project key and one Google API-key-shaped string exist in the bundle; neither is recorded. `resources/accessibility/reading_mode_gdocs_helper_manifest.json` was skipped because it carries a private key.

## 1. Corrections to the scout inventory (verified this pass)

1. The extension does not use `chrome.debugger` to drive pages. It only calls `chrome.debugger.getTargets` to map tab ids to CDP target ids; it never attaches or sends commands (`AM/background.js:7478, 8881-8887, 9552-9580`). Page control is done by the daemon over CDP (SRC, high).
2. `chrome.runtime.connectNative` is used only for the macOS Apple Passwords native host (`com.apple.passwordmanager`, `AM/background.js:8406-8619`). The "register / connectionParams / pong" messages the scout attributed to native messaging are the WebSocket extension bridge (`register`, `registered`, `runCommand`, `cancelCommand`, `ping`/`pong`) and the tRPC WS client (SRC, high).
3. "Desktop computer use: mouse.move/click/drag, keyboard.text_input/press/shortcut" is wrong as an agent capability. Those names are context-awareness event kinds recorded about the user (`source: aside_dom | mac_ax | win_uia`, plus `screen.ocr`). `AsideComputerUse.exe` is the context-awareness recorder, driven by JSON lines on stdin (`pause`, `shutdown`, `health`, `permissions`, `invoke`, `policy`, `menu`). The agent drives only the browser, plus bash and file tools. `computerUseToVertex` and `computer-use-preview` strings come from the vendored Google GenAI SDK and a model catalog entry (SRC, high).
4. "Task record and replay: session_replay" is a PostHog SDK constant (analytics session recording), not agent replay. The only replay-like assets are persisted transcripts, screenshots and an optional `page.video` recorder (SRC, high).
5. "Prompt-injection defenses: none found" is wrong. A dedicated untrusted-tool-output module exists (section 5.4) (SRC, high).
6. "Agent cursor/overlay: not found" holds. The daemon injects only the isolated `__aside` world, a console interceptor, a wait monitor, a drag polyfill and a transient numbered annotation overlay (SRC, medium).
7. "Domain allowlists: none" holds. The default permission rule is allow (section 5.1) (SRC, high).
8. `127.0.0.1:5175` is the web-dev origin; the production daemon port is 21420 and the browser CDP default is `localhost:45103` (SRC, high). `/remote-control` is an internal pseudo-path used by the remote-control multiplexer, not a public listener.
9. `aside_proxy.exe` is a versioned-payload launcher (Chromium `chrome_proxy` style), not an agent proxy: REA `inspect_pe_resources` shows only RT_VERSION and a 99,702-byte RT_MANIFEST; REA `search_strings` finds only `Aside.exe`, `aside_proxy`, `--config` and Windows version lookups. `IwaKeyDistribution` and `MEIPreload` are upstream Chromium components (a 3.5 KB bundle-id/key protobuf and an 8 KB Media Engagement protobuf) with no agent role; no IWA use was found (INF for the "launcher" label, SRC for the rest).

## 2. Architecture and process model

```
User ── side panel (MV3 extension "Aside Browsing Agent", React)  ──┐
                                                                   │ tRPC over HTTP/WS, loopback only
 Aside.dll (Chromium 153 fork)                                     ▼
  ├─ DevTools server  localhost:45103  (challenge-signed token)   aside-daemon.exe (Node SEA, 127.0.0.1:21420)
  ├─ private APIs: asideAccount, asideNotification, ...            ├─ sessions, tools, permissions, model calls (SQLite WAL)
  ├─ native UI: Agent Tabs, take over, task popover              ├─ QuickJS REPL ("Asidewright", Playwright-style, in-house CDP)
  └─ daemon supervisor (health/shutdown loaders)                  ├─ CdpClient ──► browser DevTools (Authorization: AsideSessionToken)
                                                                   │      └ "Aside.*" methods ──► extension bridge (WS /extension-bridge/extension)
 Vault extension "Aside Vault" ◄── chrome.runtime.sendMessage ◄────┤
 AsideComputerUse.exe (context awareness, stdio JSON lines)        ├─ AsideWindowsSandboxHelper.exe (AppContainer) for the bash tool
 aside-native.node (CNG/TPM key, Windows Hello)                    └─ outbound relay for Remote Control (pro/max)
```

Division of labour (SRC, high):

- Page-level control (snapshot, click, type, screenshots, dialogs, downloads, file choosers) goes daemon -> CDP -> page. The daemon attaches with `Target.attachToTarget flatten:true`, `Target.setAutoAttach`, creates a CDP isolated world per frame, and evaluates a bundled helper (`__aside`) in it (`daemon_sea_blob.bin@3036619, 3033012`).
- Browser-level control (tab groups, tabs without CDP reach, notifications, history, cookies setting, downloads wait, vault commands) goes daemon -> extension bridge -> `chrome.*`. The `CdpClient.send` routes any method starting with `Aside.` to the extension, so both families share one call path (`@2741135`).
- The extension is also the UI host (side panel, new-tab page, lasso, translate, capture). It reaches the daemon with tRPC (`/trpc`) and WebSockets using the same authorization.
- The vault is a second extension plus a daemon-side vault service; the agent never touches vault secrets directly (section 5.5).

Process and IPC inventory:

| Link | Transport | Auth | Notes |
|---|---|---|---|
| extension -> daemon | HTTP/tRPC + WS on 127.0.0.1:21420 | signed challenge -> JWT `AsideDaemonSessionToken` (24 h; extension caches 12 h) | WS subprotocol `aside.daemon.auth.<jwt>` |
| browser core -> daemon | same flow, `clientKind=chromium` | same | `aside_daemon_authorizer.cc` |
| daemon -> browser | CDP WS to localhost:45103 | challenge-signed `AsideSessionToken`, 10 s refresh margin | `/json/auth/session` POST only; Host must be IP/localhost |
| daemon -> extension | WS `/extension-bridge/extension` | same subprotocol token | `register`, `runCommand`, `cancelCommand`, `ping` |
| extension -> vault extension | `chrome.runtime.sendMessage` to pinned id | `externally_connectable` ids pinned both ways | commands `performAutofill`, `createGeneratedPasswordRef`, ... |
| daemon -> helpers | child_process stdio JSON lines | parent pid, account generation check | context-awareness helper, sandbox helper |
| daemon -> cloud | HTTPS; WSS relay for Remote Control | account + device enrollment, host token | procedures and WS paths allow-listed |

## 3. Trust and authentication design (category 7)

- Loopback only: `HOST` is forced into {127.0.0.1, localhost, ::1}; the middleware returns 403 for any non-loopback `Host` header, including on public paths (DNS-rebinding defence); CORS and Private Network Access headers are granted only to `chrome-extension://` and loopback web origins (`@1289900, @2701001, @1351704`).
- No static secrets in clients. Both the daemon and the browser's DevTools endpoint use the same shape of challenge-response: the server issues 32 random bytes (daemon: 30 s TTL, max 1,024 outstanding), the client signs `"Aside Daemon Auth v1\0" || challenge` (daemon) or the raw 32-byte challenge (CDP) with the installation key, and receives a short-lived bearer. A 401 triggers exactly one forced refresh.
- The installation key lives in native code: Windows CNG with "Microsoft Platform Crypto Provider" (TPM) and fallback "Microsoft Software Key Storage Provider", ECDSA P-256; scheme enum `p256_v1 | pq_v1` with ML-DSA-65 verification support in the daemon. A data key (DK) is unwrapped by the helper, HKDF-SHA256 derives per-purpose subkeys (for example `daemon.auth.token` for the JWT signing key), and blobs use ChaCha20-Poly1305. Windows Hello is used for vault unlock (`aside-native.node` strings). REA `open_binary`/`search_strings` on this 7.5 MB addon timed out (330 s), so these strings came from byte scans (SRC, medium on the TPM/PQ details).
- Clients are typed: `clientKind` is one of `chromium`, `agent-manager-extension`, `password-manager-extension`, `internal-web`, `cli`.
- The extension still carries very broad permissions (`debugger`, `cookies`, `history`, `management`, `<all_urls>`, `userScripts`). The compensating controls are the daemon authorization, the whitelisted `callExtensionMV3Api`, and the fact that privileged tab/window mutations are blocked in the agent's `chrome` global (`PRIVILEGED_EXTENSION_MV3_API_METHODS`, 26 methods) while a 32-method read/data list is allowed. Several allowed methods are destructive data operations (`history.deleteUrl`, `bookmarks.remove`, `downloads.erase`), and no extra gate was observed for them (INF, medium).

## 4. Agent tool protocol (category 9)

### 4.1 One tool, a scripting REPL
The model mainly calls `repl`: persistent QuickJS (WASM) with a Playwright-style API, 120 s per call, 16 KiB of output returned to context (full output kept), no modules, frozen host surfaces, non-marshalable `CdpClient`/`AsideBrowser` handles, interrupt handler for abort and deadline (`@6370931, @10810983, @10812884, @10696592`). Other tools: `bash` (sandboxed terminal), `read_file`, `write_file`, `edit_file`, `webfetch`, `websearch`, `write_todos`, `history_search/read`, `memory_search`, `notification`, `routine_update`, `subagent`, `subagent_wait`, `ask_user_question`, `request_action_confirmation`, `get_time` (gated by session type, `@11206294`).

Globals: `page`, `tabs`, `listBrowserTabs`, `attachBrowserTab`, `attachActiveBrowserTab`, `openTab`, `closeTab`, `snapshot`, `annotatedScreenshot`, `fetch` (with the page's cookies), `display`, `fs`, `aside`, plus skill clients (`passwordManager`, `externalPasswordManagers`, `captcha`, `cua`, `chrome`, `channel`, `slack`, `notion`, `googleDocs`, ...). The Playwright-like classes (Page, Locator, Mouse, Keyboard, Download, FileChooser) are an in-house CDP implementation; no playwright/puppeteer import exists (SRC, high). The internal codename in skills is "Asidewright".

### 4.2 Observation: ARIA snapshot with refs
An injected script (`src/browser-v3/injected/aria-snapshot/*`) builds a compact accessibility-style tree. Refs are `e<N>` in the main frame and `f<k>e<N>` for iframe k. Refs attach to iframes, canvases, scrollables, interactive nodes (tag, onclick, tabindex, cursor:pointer heuristics) and named content roles. Options: `interactive`, `showHidden`, `ref`, `selector`, `maxDepth`, `maxChars`. The REPL returns `{tree, diff}`; the system prompt makes snapshot the primary reading method with an escalation ladder (interactive snapshot, full snapshot, wait, annotated or raw screenshot). Each snapshot invalidates older refs; stale refs are re-resolved by role + name + ordinal (budget 5,000 nodes) or fail with an actionable error. Per-frame snapshots are serialized and retried (3x). Password inputs always show `[redacted]` (`@2992684, @3018489, @3137910`).

### 4.3 Action: actionability, trusted input, settle logic
- Actionability (`waitForReady`: attached, visible, enabled, stable, 5 s), hit-target test through shadow roots, label-to-control retargeting, custom checkbox/radio/switch handling, DOM-activation fallbacks with state verification (`@3018489, @3075397`).
- Trusted CDP input events with modifiers and double click; file chooser interception; JS dialogs auto-accepted (ref-counted) with a `[system]` transcript note.
- Settle logic after actions: quiet DOM window 350 ms (mutation ratio <= 1%), same-site network tracking (budget 1.2 s), SPA URL dwell 500 ms + 750 ms, redirect guard 1 s, hard cap 8 s; WebSocket/EventSource/Ping/Prefetch ignored (`@3061661`).
- Vision fallbacks: `annotatedScreenshot` (numbered red boxes drawn into the page for the capture, removed after) and the `cua` coordinate API documented in the `visual-browse` skill, with rules to return to refs quickly.

### 4.4 Tabs, windows and background browsing
- `openTab` creates inactive tabs next to the session's anchor tab and puts them into a native tab group titled `Agent Tabs`. Tabs are `owned` (agent-created, closed at cleanup) or `borrowed` (the user's tab attached on request; closing only detaches). Table `session_tabs {id, sessionId, ownership, source (open_tab | popup | attachment), targetId, url, data}`.
- Each session stores `browserBinding {profileId, browserMode default|incognito, windowId, anchorTargetId}`. Window resolution returns structured outcomes (`ok` via last-focused / single-window / focused-flag; `no-normal-window`; `tabs-without-normal-window`; `ambiguous-windows`) that become user-readable errors. Commands carry a route and the extension rejects wrong profile or mode.
- Background execution: per page session the daemon sets `Page.setWebLifecycleState active` and `Emulation.setFocusEmulationEnabled`, and uses a fixed 1440x900 viewport for agent tabs (`@3035913`).
- Lifecycle: tabs of idle sessions close after 15 minutes (configurable 5-1440); on each extension connect, orphan tabs are cleared and affected sessions get a system message that tabs from the previous process were closed.

### 4.5 Sessions, steering and recovery
- Versioned WS protocol `/agents/chat/:sessionId?runId&afterSeq`: commands `prompt | continue | steer | queue | interrupt` with idempotent `commandId` ledger (conflict if reused with a different payload), events with `runId` + `seq`, `expectedTurnId` guard, payload caps (32 messages, 256,000 chars). Setting `followUpBehavior = queue | steer`.
- Session status: `idle | running | suspended | errored | interrupted | aborted`; ephemeral sessions purge 15 minutes after terminal state; runs left `running` by a crashed process are resumed at start-up; suspensions that already carry a response are re-entered, and unresolved ones wait for the UI (`@11284734, @6596973, @11370627`).
- Subagents: up to 8 (default, hidden custom, `context_explorer`, read-only `code_explorer`, `fork_self` sharing the prompt cache). Subagents cannot see the parent's tabs or REPL state and cannot ask the user. Global cap of 200 concurrent runs.

### 4.6 Hooks and harness versions
Policy and side effects sit in a `HookRegistry` with 16 hook points (`tool.execute.before/after`, `repl.execute.before/after`, `repl.context`, `browser.access`, `browser.navigation`, `browser.download`, `file.access`, `network.access`, `chat.message`, `agent.turn.completed`, `agent.final_response`, `subagent.run.started/ended`, `session.dispose`). Loaded hooks: context recall, skills, subagent, permission enforcement, task completion notification, memory. Harness `v1` is the browsing REPL agent; `v2` is a tool-light agent whose only user-visible output is `send_message(progress | final | question)`.

### 4.7 External agents and remote control
- CLI router `cli.replStart | replRun | replState | replClose | replDescription | skillsList | skillsShow` exposes the same REPL to external agents as an ephemeral incognito-style session, headless (every `ask` becomes deny), idle TTL 30 min (`@11366603`).
- Remote Control (pro/max plans): the daemon enrolls as a host, keeps an outbound WSS connection to the cloud relay, and serves only 27 allow-listed tRPC procedures (including `sessions.resolveSuspension`, which means approvals can be given remotely, and `cli.repl*`) and four WS path shapes; payloads over 100 KiB are segmented (32 MiB cap) (`@11379242, @11728027`).

## 5. Safety, permissions and credential handling (categories 9 and 7)

### 5.1 Permission engine
Session modes `read-only | guard (default) | full-access`, merged with an account-level `permission` setting and a per-session override. Rules are matchers of type `tool` (glob, optional argument eq/regex), `browser` (action `read | modify | download`, optional URL glob) and `network` (URL or domain glob). Precedence: deny, approved, ask, allow, default. Files use readable/writable roots compared after realpath resolution and `outsideRead/outsideWrite = deny | ask`. Defaults: rules default `allow`; guard allows the account root, runtime dir, task dir, Documents and Downloads and asks elsewhere; sandbox on. The enforcement is a hook (`PermissionEnforcementHook`) so every tool call, file access, browser action and network call passes one function (`@6599698, @6591993, @2565669, @1354079`).

Practical effect: modes mostly protect the filesystem. Browsing and network access are unprompted by default in every mode; the real brake on risky web actions is the model-driven confirmation tool (5.3).

### 5.2 Durable suspensions
Approval, `ask_user_question` and action confirmation all use one persisted suspension on the session row (`{kind, toolCallId, request, createdAt, response|error, resolvedAt}`), one at a time. Responses: approval `{verdict allow|deny, always}`, question answers, confirmation `{verdict confirm|cancel, instruction}`. Denying with text rejects the suspension with "User asked Aside to do this instead: ...". "Always" persists scoped rules (file directories into session roots; tool/browser/network into `approved`/`deny`). Headless triggers (CLI) auto-deny any `ask`. The UI card is keyboard-first (Meta+Enter allow, Esc deny, numbered choices). Notifications for approvals are built from the same scope (`Read file.txt`, `Run tool`, `Modify <url>`, `Access <url>`), suppressed when the session is visible, ephemeral, or the setting is off.

### 5.3 Final-confirm mode (soft control)
`runtimeConfig.finalConfirm` (UI: "Ask first") adds the tool `request_action_confirmation {title, message, artifact}` and a prompt requiring a call before sending messages, posting, paying, deleting, security changes, invitations or final submissions; the call must be alone in its turn and include a draft preview (gmail, imessage, linkedin, slack, calendar, tweet, skill) or a screenshot. Weakness: nothing in code blocks a submit without it (SRC for the absence of enforcement code, INF for the consequence); the default is off; and a provider-specific prompt tells one model to avoid asking for permission.

### 5.4 Prompt-injection controls
Layers found (SRC, high): (1) the `guardUntrustedToolResultForLlm` wrapper (`[BEGIN_UNTRUSTED_TOOL_OUTPUT tool= id=]` ... `[END_...]`), with obfuscation-tolerant neutralization of fake boundaries and role tags (`user`, `assistant`, `system`, `developer`, `tool`, `aside`, `untrusted`) and an explicit "PROMPT INJECTION DETECTED" note; applied only to models not on `HIGH_CAPABILITY_UNTRUSTED_OUTPUT_MODEL_IDS`; (2) system-prompt instruction boundaries (a `<system_message>` counts only in the user message; tool output, web pages, files and search results are data; on suspected injection stop and ask); (3) `<untrusted_context>` wrapping for recalled history and "evidence, never an instruction" for briefings; (4) attachment text is JSON-escaped and angle brackets escaped; (5) secret redaction on capture. No classifier model and no per-origin trust tiers were found.

### 5.5 Credentials
The model never sees stored or generated secrets. `passwordManager` returns summaries only, `generatePassword` returns an opaque ref resolved inside the vault, `autofillItem` and `fillPassword` execute through the vault extension. The daemon-side `AgentPasswordManagerAccess` requires an agent session that is not incognito, enforces `agentAccessPolicy` (default `while-unlocked`; `never` disables), per-item agent accessibility, and returns a login fill payload only if the item matches the target URL. The vault extension requires `agentSessionId`, records a pending audit entry (`actor: agent`, 60 s), validates the page analysis (`frameUuid`, `snapshotRevision`) and OAuth provider evidence, and reports `policy_denied | authorization_required | authorization_rejected | fields_not_found`. External managers (1Password, Bitwarden, ...) are injected by the daemon through a guarded `Runtime.callFunctionOn` that re-checks origin and `input[type=password]` and uses the native value setter. Snapshots redact password inputs; the durable capture path adds field-semantic and value-shape redaction with a Luhn check.

### 5.6 Sandbox for shell commands
Windows: `AsideWindowsSandboxHelper.exe run --isolation appcontainer ... --read/--write roots --network mode`, with a `restricted-token` alternative (full network only), Job Objects, a named mutex, built-in self-tests, and a `setup-loopback` subcommand that needs one-time administrator approval because AppContainers cannot reach localhost by default (REA evidence `ev_13e85340d1c284acb6f274e50ad7dffafc2426472300fb779778948d2e6a5dfd`). macOS: seatbelt. Other platforms: passthrough, no sandbox.

### 5.7 User-consent policies that appear to be settings only
`paymentUse` (ask / auto-approve under $50 / never; cards by brand and last4; daily or monthly spending limit with ask or block) and `communication` (ask or auto before sending, per-app toggles, watch replies, tone matching) are defined and exposed in settings UI, but no daemon code consumes them (SRC for absence in the daemon; enforcement location unknown, INF).

## 6. How agent actions are surfaced to the user

- Native browser chrome: a tab group `Agent Tabs`; strings and classes for "Aside is controlling the tab" and a "Take over" button (`AsideAiTabsTakeOver`, `AsideAiTabsViewport`, `AsideAgentTabsButton`, `AsideAgentSessionTabsView`); task popover, status dots and attention badge (`AsideTask*`). No daemon code reacts to take over (SRC absence); user control is via interrupt/steer or the native control (INF).
- Side panel: transcript with tool-call rows, to-do list, search results, artifacts/files, approval and confirmation cards, draft previews.
- Notifications through the bridge (`Aside.showNotification`, `requireInteraction`, click path to the session) for approvals, questions, confirmations and completions, with suppression rules.
- Final proof: the system prompt requires a screenshot as visual proof for browsing tasks; `takeScreenshotOnEverySnapshot` optional.
- Not found: a page overlay cursor or element highlight (only the transient annotation overlay used for screenshots).

## 7. Notable design decisions

1. Hybrid control plane: CDP for the page, an extension bridge for browser state, one routing client.
2. Capability-secured local services: signed challenge for both the daemon and CDP, loopback+Host checks, hardware-backed key.
3. One scripting tool instead of many atomic tools; guardrails live in the runtime (warnings, stable refs, hit-target tests) and in hooks.
4. Policy as hooks with typed rule data, and human decisions as persisted suspensions that survive restarts and can be answered remotely.
5. Credential indirection (opaque refs, URL-bound fills, audit attribution) rather than "ask the model not to leak".
6. Background-first agent: hidden tabs, forced active lifecycle, idle reaping, notifications to pull the user back in.
7. Same runtime for built-in agent, subagents, CLI/external agents, routines and remote devices, differentiated by trigger type (user, subagent, routine, channel, cli, remote) and session flags (incognito, ephemeral, headless).

## 8. Best in class

- Authenticated local control surfaces (challenge-response bound to a TPM-backed key for both daemon and CDP) - rare among AI browsers (REPL for external agents often opens an unauthenticated debug port).
- Durable approvals and run recovery, with "deny with instruction" and remember-scope.
- Agent-safe credential handling (vault refs, URL-matched fills, audit attribution, origin-bound injection for external managers).
- Snapshot ergonomics: interactive-only trees, frame-prefixed refs, ref re-resolution, post-action diff, tuned settle heuristics.
- Tab hygiene: owned vs borrowed tabs, named group, idle close, post-crash reconciliation.
- Steering protocol: idempotent commands, sequence-numbered events, steer/queue/interrupt.

## 9. Weaknesses and limits

1. Open defaults: browser and network rules default to allow; no sensitive-site list; no per-click approval; Linux has no shell sandbox.
2. Self-editable guardrails: the `aside` REPL global can write any setting except three keys and can update a session's `permissionMode`, `permission` and `runtimeConfig.finalConfirm` (SRC for the code path; timing of effect and any extra check not verified; medium confidence).
3. Final-confirm and consent policies are prompt-level or settings-only; no hard enforcement seen in the daemon.
4. Prompt-injection wrapper is skipped for frontier models and is heuristic; no origin-trust model.
5. Blanket auto-accept of JS dialogs and cookie transfer into a Node `fetch` widen the blast radius of a hijacked agent.
6. Extension privilege is very broad; some destructive read/data MV3 methods are allowed.
7. Remote Control lets remote callers answer approvals and start REPL sessions; trust rests on the cloud relay (not visible here).
8. Context awareness captures typed text by default (privacy), though it is plan-gated (max), excludable and redacted.
9. Windows-centric native pieces; ML-DSA use on Windows is unverified.

## 10. What Verevon should copy, adapt or avoid

Verevon reference points (from the t2 baselines): Quarry-owned sessions with `allowed_domains`, step/time budgets, AX-ref `browser.observe/act`, BrowserBroker grants (in-memory, unissued in production), risk gates (login, checkout, posting, destructive, cross-domain, persistent cookies), human takeover with 409 to agent actions, ZDR refusal, no vault, no cursor overlay, no run replay.

Copy (high value):
1. Durable suspension model for approvals: persist `{kind, toolCallId, request, response}` on the run record; one pending decision per run; re-entry after restart; "deny with instruction"; "remember this scope" writing a scoped rule (replaces in-memory BrowserBroker grant state).
2. Agent-safe credential boundary for Quarry: opaque generated-password refs, URL-bound autofill payloads, agent-session attribution in audit, origin and field-type re-check at injection, policy `never | while-unlocked`.
3. Signed-challenge authentication for any local or attach-to-user-browser channel; Host-header allowlist and PNA headers.
4. Post-action settle defaults (quiet DOM 350 ms, same-site network budget, SPA dwell, 8 s cap) and ref re-resolution by role+name+ordinal.
5. Owned vs borrowed tabs, a named agent surface, idle reaping, post-restart reconciliation with a recovery message to the model.
6. Steering protocol: `commandId` idempotency, `afterSeq` resume, `expectedTurnId`, steer/queue/interrupt.
7. Typed rule vocabulary for browser policy: action x URL-glob matchers with deny > approved > ask > allow precedence; headless = ask->deny.
8. Preview artifacts attached to confirmations (draft or screenshot), and final visual proof for unattended runs.
9. Controlled-tab indicator with one-click take over in the in-app browser.
10. Runtime lints and actionable errors in tool results.

Adapt with care:
- Scripted REPL tool: only behind the same broker grants and with a WASM interpreter and marshalling allowlist; keep `browser.act` ref-only for the default planner.
- Final-confirm: keep Verevon's deterministic risk categories as the enforcement and use model-requested confirmations only as an additional layer with a preview.
- Remote approvals: allow-list procedures and force account scoping, but require step-up for destructive or paid actions.
- Per-model prompt overrides: avoid prompts that discourage asking for permission; keep approval enforcement in code.

Avoid:
- Letting any model-callable surface edit permission, consent, ZDR or grant settings (`aside.settings.set`/`sessions.update` pattern).
- Default-allow browsing without domain scoping; Verevon's first-host `allowed_domains` is better.
- Blanket auto-accept of confirm dialogs; keep explicit `respond_dialog` with a gate.
- Skipping the injection wrapper for stronger models.
- Silently granting browser-level permissions (notifications) for the agent.
- Shipping CAPTCHA-solving as a default skill without a compliance decision.

## 11. Open questions

1. Does `aside.sessions.update` or `aside.settings.set('permission', ...)` take effect for the running session and is there an approval prompt in the UI for such a change?
2. Where are `paymentUse` and `communication` policies enforced (cloud API, prompt injection of settings, skills, or not at all)?
3. How does the native "Take over" interact with the daemon (does it interrupt the run, pause tool execution, or only focus the tab)?
4. How does the browser verify the CDP challenge signature (which stored public key, which scheme) and is `pq_v1` used on Windows?
5. Is there any org-managed policy channel for permission rules (Aside Enterprise strings exist)?
6. Does the browser use Chromium's "Allow remote debugging?" consent for the daemon's CDP connection or bypass it with the signed challenge?
7. What does `capture-tab-without-userinteraction` capture and who calls it?
8. How is ffmpeg provisioned for `page.video`?
9. Harness v2 consumers (channels, cloud runtime) and how the cloud runtime (`ASIDE_RUNTIME=cloud`) gets a browser.
10. The REA JavaScript graph for the daemon bundle was partial (worker heap exhausted in the semantic stage; 0 modules, 636 findings); trace queries for `request_action_confirmation`, `/auth/daemon/challenge` and `WebSocket` returned no match. All behaviour claims therefore come from direct source reading.

## 12. REA tools actually used

- `analyze_javascript_application` (format directory, `max_heap_mb` 2048, `integrity_policy` record-and-continue, 600 s) on the extracted daemon bundle: retained evidence `ev_21247f0c84c8a173da6b0b554798de29ce5cff11a801691ee8fb9c397427c493`, summary view `ev_0cd8120a01074ae3ce66bda42022353a00d915e44c3fc6c7cfb3c79232bdef7a`; coverage partial.
- `trace_application_feature` x3 on that evidence (string `request_action_confirmation`, route `/auth/daemon/challenge`, api `WebSocket`): all `no-match` because the graph is partial (`ev_106f70f4...`, `ev_304085f2...`, `ev_4749a554...`).
- `inspect_pe_resources` on `aside_proxy.exe`: `ev_2c158404d56a4b4dbbc05a4cf225cb22206de44062fb635a04e17ce2a22a1a65`.
- `open_binary` + `search_strings` + `close_binary` on `aside_proxy.exe` (regex result saved to a session file and filtered with node) and on `AsideWindowsSandboxHelper.exe` (`ev_13e85340d1c284acb6f274e50ad7dffafc2426472300fb779778948d2e6a5dfd`).
- `open_binary` + `search_strings` on `aside-native.node`: timed out after 330 s (provider_timeout); closed; substituted by byte scans.
- Everything else was direct reading with grep/node over installed program files, locale paks (parsed with a small pak reader), extension sources and the daemon payload.
