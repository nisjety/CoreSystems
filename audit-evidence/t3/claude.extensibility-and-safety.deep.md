# Claude desktop 2.31226.1: extensibility and safety deep dive

Audit 3, facet "extensibility-and-safety". Read-only static analysis of the extracted Electron app (Windows MSIX, Electron 44.4.3) plus public Anthropic docs. Nothing was launched, installed or modified. Date of audit: 2026-10-13. Companion file: `claude.extensibility-and-safety.features.json` (153 records, ids `claude-extensibility-and-safety-001` to `-153`; cited below as F001 to F153).

Provenance tags: SRC = read directly in the extracted files, REA = observed through REA analysis of a shipped artifact, DOC = public documentation, INF = my inference (basis stated).

## 0. Scope, method and how to reproduce

**What the app is.** A thin Electron shell around the remote claude.ai web UI plus a large privileged local layer: a main process, utility-process hosts, an embedded Claude Code engine (driven through the Agent SDK over stdio), an optional Linux VM sandbox (Cowork), and bundled MCP servers. The web UI renders chat; almost every safety and extensibility control lives in the local layer. [SRC, REA]

**Sources read.**
- Main-process chunks, pretty-printed for line citation (token-based line breaker, `tmp/claude-ext/pretty.cjs`, output in `tmp/claude-ext/pretty/`). Evidence strings of the form `pretty/<file>:<line>` refer to those copies. Mapping: `core.js` = `.vite/build/index.chunk-EzXl5k4A.js` (7 MB main logic, policy registry, session manager shell); `sdk.js` = `index.chunk-D7ye_MQA.js` (Claude Code desktop session manager and permission broker); `c1xGi.js` = `index.chunk-1xGiXs9t.js` (MCP gate, desktop-control tool servers, Cowork classifier rules); `cBeq.js` = `index.chunk-BeqXJNqE.js` (Cowork session manager); `c_D6Q00DJz.js` = `index.chunk-D6Q00DJz.js` (embedded Claude Code settings, plugin and SDK runtime); `cDBiW.js` (audit log); `c_C6_NM1s9.js` (tag scrubber); `c_CR1LIe8r.js` and `c_CH3m0eAL.js` (GitHub built-in connector); `o365.js` = `resources/office365-mcp/office365-mcp-stdio.mjs`; `mcp-runtime/*`.
- Line numbers in pretty copies are stable only for those copies. `verify.cjs` and `verify2.cjs` (in `tmp/claude-ext/`) check that every cited range exists and that about 100 anchor regexes still hit the cited lines (0 failures at the end of this run).
- Public docs read via WebFetch on 2026-10-13: permission modes, auto-mode configuration, MCP, mods overview, Electron fuses.
- Verevon baselines in `findings/t3/v-modelplane.baseline.md` and `v-chat-agents-ui.baseline.md` for the gap analysis (section 11).

**REA usage log (this run).**
- `analyze_javascript_application` (directory, summary, record-and-continue): on `resources/office365-mcp/` (the bundled Microsoft Graph MCP server): evidence `ev_f95eca018628b91e6f022ae42e52179e2249a15fa91d78cd6c768f0f08306611` (parent `ev_6a65057a...`): 1 parsed file, 1,477,211 AST nodes, 246 findings, semantic pass hit the worker heap limit (partial coverage). On the 6-file runtime subset (`mcp-runtime/*`, `mainView.js`, consent and artifact preloads): `ev_da1dab835cbb36e19955e3a2eebc2e036760d4ac5a80ee66715538c6294fa7fe`: 26 contextBridge APIs, 1,373 IPC operations of which 1,365 dynamic, semantic pass partial. This reproduces the counts the earlier attempt recorded in F023.
- `trace_application_feature`: module seed `nodeHost.js`: `ev_210035cbba2aedab1c5282527ee32d57b69c4df68a1d5e9ea9c534cb18cdad18` (2 seed matches, 19 nodes, 17 import edges; reproduces F024). String seed `Mail.Send` on the Microsoft 365 bundle: no match (`ev_9306694d...`), although `grep` finds four hits. Lesson: REA literal seeds do not reach string literals in large bundled files; treat a no-match as absence of graph coverage, not absence of the feature.
- `inspect_pe_resources` on `claude.exe`: `ev_da0c1d6fc37369a429db6c8d6ce05ed6d4a966f99330ad7b61c267e4d8c667c5`: PE32+, 246,402,896 bytes; an `INTEGRITY/ELECTRONASAR` resource holding the ASAR integrity digest (F145).
- Earlier work (recorded in F048): the scout pass full-app analysis `ev_cecd2db7e6...`. REA keeps no retained evidence across calls (`get_evidence_bundle` returned 0 records at the end of the run), so IDs are citations of produced results, not re-queryable handles.
- Not used: `open_binary`, `binary_session` (avoid the shared active-target session).

## 1. Executive summary: the ten things worth copying

1. **A layered action-decision pipeline that always fails toward a human**: deny/ask/allow rules with a strictness lattice, then pre-execution hooks (confinement, canonicalisation), then a host broker that computes a "stand-down verdict" (card, classifier or bypass) with a typed cause, then an optional result-blind LLM classifier, then a transport-level MCP gate. F001, F002, F073, F121, F123, F130, F148.
2. **Approval cards are treated as an attack surface**: recipient-first anti-decoy rendering, invisible-character stripping, single-use receipts bound to the SHA-256 of the exact arguments, nonce-bound self-management consents, app-prepared fingerprinted plans re-checked at execution, forged-card defences. F029, F074, F117, F039.
3. **Host-authored classifier policy**: the app tells the auto-mode classifier how to judge its own tools with about 46 KB of natural-language rules in a uniform grammar ("must name ... clears when ..."), merged with the CLI's `$defaults`. F114, F115, F116, F118 to F120, F146.
4. **A typed policy registry (183 keys) that is the single source of truth** for validation, admin UI, redaction, egress declarations and fail-closed values; remote config is tighten-only until verified; one schema serves the desktop, CLI and Microsoft 365 add-in readers and a compiler projects it onto the engine. F012 to F015, F066, F084, F085, F109.
5. **Trust-source gating everywhere**: executable config (helper scripts, MCP commands, plugin OAuth hosts) is honoured only from admin sources; repository-resident settings can tighten but never widen; imports drop auto-executing config files. F037, F086, F127.
6. **First-party connectors designed for blast-radius control**: tools register only if the delegated scope was granted; write tools default to ask; eight send/merge tools per connector can never be set to allow; per-user velocity limits, recipient caps, sanitised HTML, AI-origin header, content-free audit. F128 to F134.
7. **Prompt-injection defences are mostly structural**: result-blind classifier, argument-provenance rules, tag-lookalike scrubbing, labelling of host-injected context as non-instruction, reserved internal names, manifest-pinned MCP prompts, URL-elicitation guard. F027, F028, F052, F076, F102, F103, F116, F118, F125.
8. **A rich extension model with supply-chain controls**: MCPB extensions with signature and org allowlist, plugin marketplaces with commit-SHA or digest pins, archive/command/subdir sources, relevance signals, in-process "mods" with a statically listable API, channels behind a managed opt-in. F033 to F035, F055, F090, F137 to F143.
9. **A bidirectional typed control protocol** between host and engine (53 host-to-agent and 13 agent-to-host request kinds) whose approval request/decision schema is directly reusable. F135, F136, F150.
10. **Transparency as a product feature**: a cause enum with plain-language "why am I being asked", tamper-evident per-session audit with decision bases, a closed vocabulary of 165 approval and lifecycle events with a named reason for every auto-deny (F153), itemised diagnostic bundles with per-section consent, a declarative egress catalog that generates CSP and firewall lists. F069, F094, F107, F122.

## 2. Trust topology: where each control lives

| Zone | Trust | Controls found |
|---|---|---|
| Remote claude.ai web UI (WebContentsView, preload `mainView.js`) | Untrusted-by-default content from a trusted origin; can call native services only from the top frame of an allowlisted origin | Sender-validated typed IPC (eipc) enforced in preload and main (F048; the sibling runtime records cover the origin allowlist in preload and main); 52 contextBridge APIs gated per origin; renderer-writable settings allowlist (F060); policy state hidden from the renderer (F036) |
| Local chrome windows (about, quick entry, consent) | Trusted, `app://localhost` | Dedicated consent window with top-frame-only preload for bootstrap-delivered code-executing config (F015) |
| Main process | Privileged | Policy registry and readers, broker, gates, safe-fs (F050), telemetry scrubber (F041) |
| Utility processes | Isolated | Direct MCP host for remote servers with OAuth (F025); Node extension host with self-exec guard (F024); workers for MSAL, PTY, transcript search |
| Embedded Claude Code engine (child process) | Semi-trusted; actions are mediated | Managed settings compiled from org policy (F066); `canUseTool` and hooks callbacks (F136, F148); env allowlist and secrets never in env (sibling runtime records) |
| Cowork VM (Linux, Hyper-V on Windows) | Sandboxed | Egress allowlist, EDR hook, approved-token proxy (F051); credential masking and injection proxy (F087); network and filesystem policy engine (F088) |
| Artifacts, previews | Hostile content | Deny-all session, request allowlist, fd-checked serving (F045); per-use connector bridge policy (F046) |
| Third-party MCP servers, extensions, plugins | Untrusted code | Signature + allowlist + blocklist (F034, F035); reserved names (F028); trust-source gating (F037); marketplace pins (F055, F140, F141) |

[SRC unless noted.] The important architectural decision: the web UI never holds authority. It can request, display and answer, but policy, grants, secrets and process control stay in the main process.

## 3. Policy plane (enterprise governance)

**Typed registry.** About 187 `flatKey` declarations (183 distinct) between `core.js:16910` and `core.js:21625`, each a zod schema with metadata: scopes (1p/3p), readers (desktop, code CLI, Microsoft 365 add-in), category and group, localisable title and description, default or display-only, predicates (show, warn, locked, required), legacy key with a documented effect, deprecations, `failClosedValue` (63 keys), redaction, egress requirements and remote-delivery policy (`requiresUserConsent`, `applyUnverified`, `hotApplied`). From one declaration the app derives validation, admin form, docs link, telemetry redaction, firewall hosts and the value substituted when a managed value is unreadable. F012, F013, F063. [SRC]

**Tiers and delivery.** Windows GPO registry (HKLM beats HKCU), macOS MDM profile, `/etc/claude-desktop/managed-settings.json`, a local config library, and an optional bootstrap server with OIDC or header auth. Unreadable managed config disables local settings rather than falling back. Bootstrap-delivered keys that could execute code or redirect credentials need per-user consent unless the device-managed profile sets `trustBootstrapDelivery`. Unverified remote config can only tighten (apply when equal to the fail-closed value or explicitly marked safe). F014, F015, F109. [SRC; DOC corroborates]

**Compilation to the agent harness.** `aHt()` turns org policy into the managed settings and environment variables of the embedded engine: permission allow/ask/deny lists, sandbox network and filesystem blocks, `disableBypassPermissionsMode`, `disableAutoMode`, plugin-only customisation, MCP allow and deny lists, telemetry env switches. One policy governs UI and agent. F066. [SRC]

**Merge semantics.** Restrictions versus grants invariant (a deny side that cannot be read withholds sibling allow rules), a 67-entry table of which value is stricter per key, and a strictness lattice for tool policy (blocked > ask > ask-session > allow, strictest wildcard wins, wildcard allow survives a merge only if both sources allow it). F084, F085, F130. [SRC]

**Settings source trust.** Security-critical keys are honoured only from managed, user or CLI sources; the same keys in project or local settings inside a repository are ignored. Public docs add that `autoMode` is never read from project settings for the same reason. F086, F146. [SRC, DOC]

**Compliance and privacy switches.** A sticky compliance latch (HIPAA) consulted by every capability gate (F021), account-level taint tags (F070), three independent privacy switches with disclosed consequences (F108), a declarative egress catalog generating CSP, firewall lists and local 503 fallbacks (F107), policy-driven retention with legal hold (F043), hardware-backed device attestation for remote sessions (F044), sensitivity-label gating for Office features (F105). [SRC]

**Documented profiles.** Standard, Restricted and Locked down key sets are published as illustrative bundles. F064. [DOC]

## 4. Action-decision plane

### 4.1 Layer map

```
model proposes tool call
  1. rules: deny > ask > allow (strictness lattice, glob resolution)           F017 F084 F130
  2. PreToolUse hooks (host-registered):                                        F147 F148
       allowed-folders deny, worktree write guard, project-config fence,
       unattended SendMessage guard, desktop-tool hook (canonicalise input,
       contested-server deny, forceAskVerdict)
  3. broker canUseTool: stand-down verdict {card | classifier | bypass}         F121
       typed cause (33), escalation checks, consent receipts, card integrity    F122 F123 F074
  4. (auto mode) result-blind classifier with host-authored rules               F114..F120
  5. MCP transport gate: validate frame, list-complete, unambiguous name,       F073
       cascade (disallowed > receipt > allowed/host grant/mode > card > deny)
  6. execution: sandbox / VM / browser pane with their own policy               F087 F088 F051
```

Every layer fails closed: the gate denies on callback error ("Do not retry it in this session"), unreadable managed settings keep the card, unknown tool names are refused, an unconfirmed server blocks auto-approval.

### 4.2 Pre-execution hooks as the app's enforcement point

The desktop registers `PreToolUse` hooks with the engine (F148): allowed-workspace-folders deny, worktree write guard (F147), project-config-root fence, a guard that stops unattended sessions from using the engine's SendMessage to reach other sessions, and a catch-all hook over every desktop-control tool that denies contested server names, requires explicit session ids, validates canonical hand-off inputs and consults the stand-down verdict. The worktree guard is notable: it classifies a write target against the session's own worktree, sibling worktrees, the base checkout, the shared `.claude` folder and the owner's settings, canonicalises Windows 8.3 names, device prefixes, UNC and WSL shares and POSIX per-process paths, and returns instructive reasons ("use the worktree path instead"). [SRC `sdk.js:2566-2692`, `4634-4716`]

### 4.3 The broker: cards, causes, stand-down, escalation

- **Stand-down verdict** (F121). `Xu`/`Qu` decide whether the app stands down for the classifier or for bypass. Auto counts only if the engine was spawned with classifier rules, the org has not disabled auto mode and, for remote targets, auto mode was chosen in this app (otherwise `auto_not_chosen_in_app` or `auto_provenance_unknown`). Unattended runs, non-local sessions, a fixed set of sensitive tools and every terminal tool force a card. A pending permission-mode change forces a card. Outcomes are logged as `desktop_ccd_*_auto_mode` with `classifier_ask` or `deferred_to_classifier`.
- **Typed cause taxonomy** (F122). A 33-entry map from cause code to a plain sentence ("Your organization requires approval", "This archives the session you're in, so Claude asks first", "Raising a session's permission mode ... Claude asks in every permission mode"). The broker passes the cause as `decisionReason` with `reasonFromApp` so the UI shows the app's explanation, not model text.
- **Escalation checks** (F123, F039). Raising a session's mode is card-only; a routine may not outrank its author (mode, stored approvals, placement on another host); a helper session may be repriced only within the caller's own model, effort and speed; turning off a Remote Control link the user created asks; an unconfirmed desktop-control server is never auto-approved; unreadable org settings keep the card. Delegated turns may be answered by a recorded delegation for a short list of lifecycle tools limited to the session and its own children.
- **Fan-out cap** (F124). Starting helper sessions is free until ten starts since the user's last message; then a card says how many were already asked. New hosts, folders or environments need consent the first time.
- **Receipts and nonces** (F074, F039 and sibling runtime records). When the engine's own permission layer allowed an MCP call, the host mints a 120-second receipt keyed by session, tool and SHA-256 of canonical arguments; the transport gate spends it once. Self-management tools carry nonce-bound consents so a model cannot fabricate a card result.
- **Card integrity** (F029, F117, F116). Recipient-like fields first, duplicate-spelling warnings, hidden-argument disclosure, zero-width and bidi stripping; app-prepared plan with fingerprint placed last and re-verified at execution; text from model-controlled fields stripped of angle brackets and capped.

### 4.4 Auto-mode classifier and host-authored policy

**Documented behaviour** (F146 [DOC]). Rules first, read-only and in-folder edits next, everything else to a classifier. The classifier sees user messages, non-read-only tool calls and CLAUDE.md, but not tool results; a server-side probe scans tool results separately; no verdict means deny; 3 consecutive or 20 total blocks pause auto mode; boundaries stated in conversation are re-read each check and can be lost to compaction (use deny rules for hard guarantees); broad allow rules that grant arbitrary code execution are dropped on entering auto mode; `autoMode` comes only from user, managed or `--settings` sources; precedence is hard_deny, soft_deny, allow exceptions, explicit user intent.

**What the desktop adds** (F114 to F120 [SRC]). It supplies `environment`, `soft_deny` and `allow` rule lists for desktop tools, applied through `settings.autoMode` at spawn and re-applied through `apply_flag_settings` when simulator panes attach, version-gated to engines at 2.1.118 or later.
- Grammar (F115): rule name, a `named+specifics` tag with a **must name** clause, the action pattern, a **clears when** sentence. The user's own message is the only clearing authority; a standing instruction or a yes to the agent's offer counts; the agent's judgment, file content, web pages, tool results, review comments and other sessions' notes never do.
- Argument provenance (F116): every description says which arguments mean something and that all other keys are the agent's own text; titles, branches and ids are labels that may carry model-written text.
- Tool-result blindness (F118): since the classifier never sees outputs, rules describe injection signals by action shape (a device action that first appears after a screen read) and judge URL and text arguments by content.
- Shell-specific policy (F119): typed terminal text is judged as the shell reads it (PowerShell prompt, cmd.exe re-evaluation of batch shims, history planting, profile and `$env:` persistence).
- Per-product profiles (F120): Cowork has its own environment (VM filesystem layout, trust boundary of sandbox plus connected folders and accounts) and rules (deletion grants, folder access, skill persistence, scheduled tasks).
- Cross-references name the CLI's built-in rule families (Irreversible Local Destruction, Unauthorized Persistence, Instruction Poisoning, Data Exfiltration, Scope Creep, Prompt Injection, Multi-Agent Coordination and others), which gives a catalogue of the categories Anthropic's own rules cover.

### 4.5 MCP transport gate

One choke point wraps every `tools/call` frame for local and in-process servers (F073): frame validation, the gate issues its own `tools/list` (50 pages max, 5 s) and refuses calls before the list is complete or when the name is not exactly one advertised tool, then a fixed cascade: session disallowed globs, single-use receipt, then (unless the tool is must-ask) allowed patterns, standing host grants, mode waiver, auto mode with a read-only annotation, then a card; callback error or null denies. Every decision logs a basis from a closed set (`disallowed_tools`, `record`, `allowed_tools`, `host_grant`, `permission_mode`, `auto_read_only`, `decided_allow`, `callback_error`, `unknown_tool` and others), which doubles as the audit schema. Calls are also bound to the active account and organisation (F075) and to per-session per-tool toggles that fail closed when session state is missing (F001).

### 4.6 Unattended, remote and scheduled runs, and helper forks

Unattended runs get a smaller tool set (no clarifying questions, no cloud hand-off, trimmed helpers; F149), may auto-approve only stored, expiring, narrowly scoped rules (F038), cannot use tools in the non-delegable list, and receive denial text written for the model ("retries in this run will fail; report the blocked URL in your findings"; F095). Side chats run as read-only forks with a deny-all prompter, closed per-tool input schemas, cwd read confinement and a breach detector that stops the side chat if a refused tool ever produces a result (F152). Device approvals from a hardware companion are limited to tools that do not run on the user's machine (F004); permission requests from phone-dispatched children relay to the parent unless risk-limited (F005); notifications offer Allow only for low-risk requests (F003).

## 5. Extensibility plane

### 5.1 MCP hosting modes

1. **Local stdio servers** spawned by the main process and bridged to the web UI over a `MessagePort` carrying JSON-RPC (F023); Node servers run inside the app's own Node in a utility process with `stdin/stdout` virtualised and a self-exec guard that redirects any spawn of the app binary to a refusal sentinel (F024). 2. **Direct MCP host** (utility process embedding the MCP SDK client) for remote HTTP servers in third-party mode: OAuth with dynamic client registration, elicitation, header refresh (F025). 3. **In-process SDK servers** for host tools (desktop control, preview, scheduled tasks), with fixed ids and reserved names (F028, F065, F104). 4. **Device bridge**: local servers and device tools announced to cloud sessions with device attestation (F030, F044). 5. **Bundled first-party servers** (GitHub, Microsoft 365) started only for admin-sourced definitions (F128). A transport proxy can rewrite `tools/list`, strip tools and inject host tools (F026), MCP App UI-only tools are hidden from the model (F031), per-server logs honour a privacy latch (F032), and trust tiers are visible in telemetry (F065).

### 5.2 Extensions (MCPB, formerly DXT)

Manifest versions 0.1 to 0.4, server types node, python, binary, uv, typed `user_config` (sensitive values encrypted with the OS keystore, F036), compatibility ranges (F033). Install pipeline copies to a private mode-0600 directory, verifies signature, binds the extension id to the manifest, checks the file hash against the security blocklist, re-verifies at extraction and records an install ledger (F034); org allowlist and blocklist service with cached encrypted verdicts that fail closed (F035); prompts in manifests are pinned so a compromised server cannot serve altered instructions (F027).

### 5.3 First-party connectors (new in this pass)

- **Policy entries** (F128): `server: github | microsoft365 | websearch` inside `managedMcpServers`, with validators (GitHub host https non-loopback, toolsets, read-only; Microsoft tenant, sovereign cloud, continuous access evaluation, scopes from a 25-scope catalogue tagged always/read/admin/write).
- **Default posture** (F129, F130): 35 Microsoft and 36 GitHub write tools default to `ask`; eight high-impact tools each (send, forward, event writes, Teams posts; merge, push, file create/delete, workflow runs) cannot be lowered by any explicit or wildcard allow; the admin form flags write tools with no policy.
- **GitHub server** (F131): Go binary over stdio, device-code OAuth, token encrypted and bound to a hash of host, client id and scope, validated against `/user`, read-only and toolset env controls, "needs auth" shell showing cached tools while signed out.
- **Microsoft 365 server** (F132 to F134): 50 tools (15 read, 35 write); a write tool is registered only if the account's delegated permissions include its scope; default grant is read-only; 403s become precise permission diagnostics; per-user buckets (10 writes per minute, 5 sends and 100 recipients per 5 minutes), 50-recipient cap, sanitised HTML with a link-only attribute allowlist, `X-AI-Generated` header, audit fields restricted to counts, lengths and hashed ids; token cache owned by the host, encrypted, size-capped and prefix-bound, with Windows broker sign-in.

### 5.4 Plugins, marketplaces, skills, hooks, mods, monitors, channels

- **Package model** (F056, F137): commands, agents, skills, hooks, MCP servers, LSP servers, output styles, themes, workflows, monitors, channels, `userConfig` (typed, secret-aware, exposed as `CLAUDE_PLUGIN_OPTION_*`), digest-pinned `binaries`, allowlisted `settings`, an `experimental` container, a `types` contract and an `evals` directory.
- **Sources and pins** (F055, F140, F141): github, git, url, npm, git-subdir (sparse partial clone), HTTPS archive with SHA-256 and host blocklist, command source whose text must be printable ASCII so the consent UI can show it, `headersHelper` only at install, unsupported placeholder stubs; admin policy matches by repository, owner wildcard, host regex, path regex, npm scope and registry pin with canonical URL comparison.
- **Suggestions** (F099, F142): discovery tools only propose; marketplaces attach relevance signals (cli, hosts, files read, manifest dependencies, cwd) that produce "Working with X?" prompts.
- **Supply-chain locks** (F090, F068): plugin-only customisation, known marketplaces only, sideload flags and command sources blocked, MCP allowed by URL or command.
- **Hooks** (F077, F089): 33 events; handler types command (exec form without a shell), HTTP with env allowlist for header interpolation, LLM prompt, agent verifier; managed-only locks.
- **Mods** (F143): JavaScript hooks module with `register(on)` running in the engine; all effects go through a mods API so `claude plugin validate` can list hooked events and calls before install; the desktop turns the runtime on per session; admins order managed plugins with `prependPlugins`/`appendPlugins` and a built-in guard seats outermost; public docs state mods are not sandboxed and can approve tool calls before the user is asked, but cannot restyle the permission prompt. [SRC, DOC]
- **Monitors and channels** (F138, F139): background watchers that push events to the model; MCP channels gated by `channelsEnabled` (default off on Teams and Enterprise) and an admin allowlist.
- **Skills** (F059, F093, F151): six always-on skills shipped as executable procedures with schema validators and mandatory QA gates (XSD validation, tracked-change check, LibreOffice recalculation with a zero-error rule, thumbnail and visual QA); per-skill listing overrides and a kill switch for inline shell execution; org skill bundles with hash pins; descriptions encode routing between file skills and the product's native document features.

### 5.5 Engine protocol (SDK control channel)

JSON-lines request/response with cancel and keep-alive. Agent to host (13): `can_use_tool`, `hook_callback`, `mcp_message`, `elicitation`, `request_user_dialog`, token refresh, UI helpers. Host to agent (53): mode, model, cwd with trust, settings, rewind, MCP management, plugin and skill reload, usage, Remote Control, session claim and more. Idempotent redelivery for reconnects (pending permission and dialog requests ride on responses). `can_use_tool` carries suggestions, blocked path, matched ask rule, decision reason with type and code, `requires_user_interaction`, `default_to_no`, MCP server identity; the decision carries updated input, rule updates (forced to session scope) and a permanence class. F135, F136, F150. [SRC]

### 5.6 The app as a tool surface (self-management)

About 30 in-process servers expose session, sidebar, window, connector, host, PR and settings control to the model (F104). Safety comes from classification: every preference declares whether the model may read or write it (72 keys excluded as security class), raising privilege is card-only, destructive calls are card-only, and the classifier rules above govern the rest (F039, F040).

## 6. Prompt-injection defence inventory

| Defence | Where | Records |
|---|---|---|
| Result-blind action classifier + separate result probe | CLI + server | F118, F146 |
| Rules that name which fields are agent text | classifier policy | F116 |
| Intent must come from the user's own message | classifier policy | F115 |
| Tag-lookalike scrubber for host envelope tags (46 tags, homoglyph and invisible-character aware) | `c_C6_NM1s9.js` | F125 |
| Host-injected context labelled as context, escaped, capped | ledger | F076 |
| Cross-session messages as quoted data; `@` defused across hosts | `c1xGi.js` | F126 |
| Anti-decoy approval cards, receipts bound to args hash | broker, gate | F029, F074 |
| Reserved names; manifest-pinned prompts; URL elicitation guard; auth-URL detector | MCP layer | F027, F028, F102, F103 |
| Persistence-vector denylists; exec-config drop on import | file layer | F011, F097, F127 |
| Built-in prompt guidance (flag suspected injection to the user) | system prompt | F052 |
| Untrusted-content labelling in web search results | tool | F052 |
| Mandatory approval classes no mode can waive | broker | F002, F123 |
| Origin pinning and site categories for model browsing | preview browser | F079, F080 |

Observation: there is no local classifier that reads tool results; the design bets on structural controls plus a result-blind reviewer plus a server-side probe.

## 7. Secrets, privacy, telemetry, egress

- Secrets: per-extension sensitive config encrypted with the OS keystore, plaintext fallback with a warning when the keystore is missing (weakness, F036); GitHub and Microsoft token caches owned by the host (F131, F134); sandbox credential masking with host-side injection and SigV4 re-signing (F087); plugin CLI credentials brokered from an encrypted vault so secrets never enter the VM (F057); reserved environment variables so a plugin cannot hijack interpreters (F058).
- Telemetry: closed-vocabulary scrubber with per-field rules and per-sink behaviour (F041); customer-owned OTLP export with content-capture categories (F042); three privacy switches (F108); declarative egress catalog (F107).
- Retention and audit: policy-driven session retention with legal hold (F043); tamper-evident per-session HMAC chain with redacted blobs (F094); itemised diagnostic bundles with per-section consent (F069); incident ledger (F111).

## 8. Computer use, browser and VM safety (pointers)

Per-app grants with TTL and capability flags (F008); tiered access (browsers and trading apps view-only, terminals click-only; F009); input guardrails (blocked system shortcuts, clipboard inference, chord limits; F010); protected-file and secure-field blocks (F011); site policy, origin pinning with DNS address pins, site-category service that fails open (F079, F080, F106); ticketed import of browser logins (F081); Chrome extension integration (F082); VM egress and EDR (F051); sandbox policy engine and credential masking (F087, F088); SSH host-key-first credential prompts (F100).

## 9. Platform hardening

Sandboxed windows, context isolation, no Node integration, typed sender-validated IPC (F048); layered permission handlers per window kind (F049); scoped fd-based safe filesystem layer (F050); dev-URL override behind biometric and host allowlist (F053); open-with handling that previews rather than installs (F054); Electron fuses: RunAsNode, NODE_OPTIONS and inspector flags off, cookie encryption on, app code only from the ASAR with an embedded SHA-256 integrity digest (F145) [REA + SRC]; `GrantFileProtocolExtraPrivileges` left at its default (enabled) while the app still loads some `file:` pages (INF: default retained).

## 10. Best in class, and weaknesses

**Best in class (relative to the other reference apps in this audit set).**
- Approval integrity (receipts bound to argument hashes, anti-decoy rendering, plan fingerprint and re-check).
- Typed, explainable approval causes.
- Fail-closed policy design (explicit unreadable values, restrictions-versus-grants invariant, tighten-only remote config, trust-source gating).
- Host-authored classifier policy and the "must name" grammar.
- First-party connector blast-radius design (grant-gated registration, never-allow set, velocity limits).
- Plugin supply-chain controls and the listable mods API.
- Egress catalog as data.

**Weaknesses and limits (with evidence).**
1. Windows host-run shells are unsandboxed in third-party mode and managed egress and folder allowlists are then not enforced for the shell (F067). [SRC + DOC]
2. Plaintext fallback for extension secrets when the keystore is unavailable (F036); the audit HMAC key sits beside the log under the same OS user, so it detects accidental edits but not a same-user attacker (F094).
3. Site-category lookups fail open on error (F080).
4. Mods and monitors are unsandboxed, same trust tier as hooks (F138, F143). [SRC, DOC]
5. Classifier-side limits: the classifier cannot see tool outputs, so exfiltration in a later argument must be judged by shape; boundaries stated in conversation can be lost to compaction; an additive developer `allow` can override an organisation `soft_deny` (documented: not a hard policy boundary). Use `permissions.deny` for guarantees. (F118, F146). [DOC]
6. Policy surface is large (183 keys) and several governance keys are `@next` and unreleased (tool-policy rules in CEL, external policy endpoint, Office access policies); enforcement of the CEL and endpoint keys was not found locally (F016, F105). [SRC]
7. Heavy reliance on minified feature flags and remote gates for rollout (numeric ids; names not recoverable), so local code under-determines runtime behaviour (F071, F114 notes).
8. Rule text in the classifier policy is long, hand-written prose; no local test corpus was visible. Maintainability and drift versus the CLI defaults are a risk (the rules cross-reference CLI rule names that are not in the bundle). [INF]
9. MCP server logs and process stderr may carry user data; a privacy latch mitigates but does not eliminate (F032).

## 11. Verevon: what to copy, adapt, avoid

Baseline (from `v-modelplane.baseline.md`, `v-chat-agents-ui.baseline.md`, code is ground truth): Verevon already has a signed capability-policy gate (Ed25519 evidence), durable HITL approvals, an autonomy ladder, provenance-tagged tool results with a deterministic injection scan, secret scrubbing, SSRF guards, strong ZDR and Space authority, and typed refusal codes. It is **behind** on: MCP tools cannot run (Do mode offers them but `trusted_capability_id` returns none for `mcp__*`; inline loop denies them), plugins are inert CRUD, hooks are in-memory and not applied in Do mode, risk is classified by substring on tool name, prompt-injection enforcement is "incomplete", Do mode starts without conversation history or authored instructions, scheduled runs are a single no-tool call, and approvals show as generic Approve/Reject cards.

### 11.1 Highest-value adoptions (ordered)

1. **Unblock MCP safely with a transport-level gate (F073, F074, F075, F001, F031).** Bind each MCP tool to a dynamic capability record at connect time (trust tier from the server's provenance, annotations `readOnlyHint`/`destructiveHint`, per-tool allow/ask/ask-session/blocked from org policy, F018, F130), and enforce one gate in the Rust MCP client: frame validation, list-complete requirement, unambiguous names, decision cascade with a closed basis enum, single-use receipts keyed by SHA-256 of canonical args, deny on error. This removes the biggest extensibility gap without weakening "Model proposes, Quarry executes" because the gate lives in Model Plane and uses Control-signed policy. Rust notes: a `tower::Layer` around the JSON-RPC transport; `enum Basis` serialised into audit events; `sha2` plus `serde_json` canonicalisation (`serde_jcs`); `moka` TTL cache for receipts.
2. **Replace substring risk matching with typed approval causes and a stand-down function (F121, F122, F123, F002, F039).** A pure `fn route(ctx) -> Route { Card(Cause) | Classifier | Bypass }` with a `Cause` enum, localisable text in Control Plane copy, and must-ask causes no mode can waive. Add the "child may not outrank parent" test (F123) to subagents and scheduled runs, and the fan-out cap (F124) to `subagent.task` and wide research.
3. **Approval card integrity (F029, F116, F117).** In the Frontend Plane, render recipient-like and destination fields first, flag duplicate keys, strip bidi and zero-width characters, show hidden-argument counts; in Model Plane, compute an app-authored plan fingerprint and re-verify at execution; never let model text into the card without sanitising.
4. **A result-blind action reviewer for Do mode (F114 to F120, F146).** Verevon already has a deterministic scan and a semantic shadow pass on tool results; add an action-only reviewer fed with user turns and tool calls, using per-tool policy fragments in the "must name / clears when" grammar so new capabilities (provider actions, shipping, tickets, browser) ship with their own rule text and Norwegian/English consent copy. Keep hard deny in the capability engine, not in the classifier.
5. **Tag-lookalike scrubber (F125).** Any Verevon prompt that wraps host context in tags (memory recall, retrieval notices, step summaries) should scrub user- and tool-originated text with a shared crate: NFKD/NFKC, confusable skeletons (`unicode-security`), default-ignorable stripping, a reserved-tag registry; unit-test with the confusable classes listed in F125.
6. **Typed policy registry in Control Plane (F012 to F015, F084, F085, F109).** One schema-with-metadata (serde plus `schemars` annotations) generating admin UI, validators, redaction, egress requirements and fail-closed values; remote or Space-level config tighten-only until verified; per-source key allowlists so a Space or workspace can never widen an owner-plane ACL (maps directly to the "Space access never widens" rule, F086). Compile the org policy into capability-core, sandbox-manager and browser-broker settings from one document (F066).
7. **Plugins and skills as supply-chain-controlled packages (F055, F090, F137, F140, F141, F142, F143, F151).** Start with the manifest and sources that need no runtime: skills and MCP servers with SHA-256 or commit pins, archive and git-subdir sources, `userConfig` with secret handling, org allowlists with canonical URL matching, and relevance signals for suggestions. Defer in-process "mods"; if added, require every effect to go through a declared API so a static scan can print a capability manifest.
8. **Connector blast-radius defaults (F129, F132, F133).** For provider actions (Gmail, Outlook, SharePoint), register write tools only when the connected account's scopes include them, default every write tool to ask with a never-allow set for send and delete, add per-user velocity buckets and recipient caps in integration-corev2, sanitise outgoing HTML with a tag and attribute allowlist, stamp an AI-origin header, and keep audit fields content-free.
9. **Sandbox secrets and egress (F087, F088, F051, F107).** Extend bwrap/Landlock sandboxes with an egress allowlist and credential sentinels injected by a host proxy; publish an egress catalog (destination, toggle, process, purpose) from which CSP, firewall guidance and admin UI are generated. This also strengthens the ZDR story: each catalog entry states whether it carries content.
10. **Audit and transparency (F073, F094, F122).** Per-run hash-chained audit stream including every approval decision, its cause and basis, with large payloads content-addressed; Verevon's audit outbox fix is built but undeployed, so adopt the chain format at the same time.

### 11.2 Where Verevon is already ahead (keep)

Signed capability evidence (Ed25519 JWS) and capability ranking; evidence-bound ZDR with typed refusals and per-connection waiver receipts; Control-signed Space authority; GDPR erasure consumers; tenant isolation. Claude's compliance latch (F021) and taint tags (F070) are a simpler client-side expression of what Verevon already does server-side; reuse the idea of a single latch consulted by every gate so ZDR cannot be bypassed by a feature added later.

### 11.3 What to avoid

- Do not rely on prose to enforce policy (F052's own note): structural gates first.
- Do not fail open for tenant-regulated lookups (F080); choose fail-closed with a cache.
- Do not ship plaintext fallbacks for secrets (F036).
- Do not leave the shell outside the sandbox on any OS (F067): on Windows hosts use a VM or job-object/AppContainer path or run shells only in the Linux sandbox.
- Do not let a classifier be the only control for hard guarantees; keep deny rules and capability evidence before it.
- Avoid a 183-key policy surface without profiles: ship named profiles (F064) and diff tooling.

## 12. Open questions

1. Which numeric remote flags enable server-side classifier review, the Cowork rules (`3424551112`) and the unattended tool trim (`478602036`) for a given account? Not recoverable locally.
2. Where are the CLI's built-in classifier rules (Irreversible Local Destruction and the other named families) kept, and how are they versioned against the desktop's rule text? Only a sample is public.
3. Are CEL tool-policy rules and the external policy endpoint enforced by the self-hosted session runtime or the cloud side? No evaluator was found in this bundle.
4. How is the receipt TTL (120 s) chosen, and is there any receipt replay protection across processes? Only the in-process ledger was read.
5. The Chrome extension's own prompt-injection and permission logic is remote (Web Store) and not observable here.
6. The Windows broker runtime (`msalruntime.dll`) and the Go GitHub server binary were not analysed.
7. How is the WSL case handled for plugins and mods (docs say mods do not run in WSL sessions)?

## 13. Appendix

### 13.1 Record index by category (counts)

Safety, permissions & governance 98; Tools, plugins & extensibility 37; Transparency, observability & control 7; Agent runtime 5; Computer & environment control 3; Models & routing 2; Platform architecture 1 (total 153; recount with `node` if the features file is edited).

### 13.2 New in this pass (F112 to F153)

F112, F113 were completed from the interrupted attempt (refusal fallback; Quick Write privacy). F114 to F120 host-authored classifier policy and its grammar, provenance, plan fingerprint, result blindness, terminal and Cowork profiles. F121 to F127 stand-down routing, cause taxonomy, escalation checks, fan-out cap, tag scrubber, cross-session envelope, exec-config drop. F128 to F134 built-in connectors. F135 to F144 SDK protocol, approval contract, plugin manifest, monitors, channels, plugin sources, marketplace matchers, relevance signals, mods, MCP layering (DOC). F145 to F151 fuses (REA), documented auto-mode (DOC), worktree guard, hook chain, unattended trim, dialog broker, bundled skills. F152, F153 side-chat read-only fork and approval telemetry vocabulary.

### 13.3 Method notes and caveats

- Minified chunks: names such as `Xu`, `Qu`, `Gl`, `Kl` are minifier output. Where set membership was inferred from neighbouring definitions, the record says so.
- Several rules, dialogs and flags use product codenames (violin-bow, lanyard, chicago, epitaxy). Purposes are inferred from fields.
- Quotes from prompts and docs were paraphrased; no long text is reproduced.
- The extracted copy is the Windows MSIX build; macOS and Linux variants (Swift helper, Touch ID, bubblewrap) were not available.
