# Codex desktop in-app browser and computer use: automation and safety deep dive (Audit 2, facet "automation-and-safety")

Read-only analysis of the OpenAI Codex Windows desktop app (MSIX `OpenAI.Codex_26.1007.2314.0_x64`, app version 26.1007.21434). Companion file: `codexbrowser.automation-and-safety.features.json` (141 records, ids `codexbrowser-automation-and-safety-001..141`). Record numbers in this document refer to those ids (for example "R008" = `...-008`).

Provenance tags: **SRC** = read in shipped code or strings; **DOC** = packaged docs, skills and policy markdown; **REA** = observation made through the REA MCP tools; **INF** = my inference, basis stated. Confidence is stated where it is not high.

## 0. Method, evidence legend, limits

### What I read
- Plugin runtime (the part that enforces policy for browser use): `plugins/openai-bundled/plugins/browser/scripts/browser-service.mjs` (1.45 MB) and `browser-client.mjs`, plus all plugin docs, `api.json`, skills, hooks and manifests. I used a beautified copy so I could cite line numbers.
- Desktop host (main process): `.vite/build/main-BklS_2Y2.js` from the pre-extracted asar (beautified copy).
- Computer-use SDK: `resources/cua_node/bin/node_modules/@oai/{cua,cua-repl,sky,browser-desktop}` (JavaScript, docs, environment docs) and the native helper `@oai/sky/bin/windows/codex-computer-use.exe`.
- Native strings of `codex.exe` (Rust core), `codex-command-runner.exe`, `codex-windows-sandbox-setup.exe`, `codex-windows-sandbox-service.exe`, and the Chrome `extension-host.exe`. These are string tables only; code was not disassembled.
- The scout inventory (`codexbrowser.inventory.md`). Where I relied on it I re-verified in code; corrections are listed in section 11.
- Verevon baselines written by other auditors (`v-quarry.baseline.md`, `v-browser-app.baseline.md`) to ground the recommendations. I did not re-verify Verevon code.

### Evidence path legend (used in the JSON `evidence` fields, always absolute)
- `SVC` = `<SCRATCH>/tmp/cxas/pretty/browser-service.pretty.js` (beautified copy of `<PLUG>/browser/scripts/browser-service.mjs`; line numbers are for the beautified copy).
- `MAIN` = `<SCRATCH>/tmp/cxas/pretty/main.pretty.js` (beautified copy of the extracted `.vite/build/main-BklS_2Y2.js`).
- `CXE` = `<SCRATCH>/tmp/cxas/pretty/codex_exe.strings.txt` (ASCII strings of `<INST>/codex.exe`; line numbers are for the strings file). Same idea for `CCU` (codex-computer-use.exe), `CMDR` (command runner), `SBSETUP`, `SBSVC`, `EXTHOST` under `<SCRATCH>/tmp/cxas/`.
- `PLUG` = `C:/Program Files/WindowsApps/OpenAI.Codex_26.1007.2314.0_x64__2p2nqsd0c76g0/app/resources/plugins/openai-bundled/plugins`; `INST` = `.../app/resources`; `CUAN` = `INST/cua_node/bin/node_modules/@oai`; `ENVDOCS` = `CUAN/browser-desktop/environment-docs`.
- `SCRATCH` = `C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-AppData-Roaming-Claude-scratch-workspaces-5e23d905-dd91-4518-8ff4-252efcbe0520-b5ff8b3a-437c-4499-9a7f-469de994f319-scratch-2026-10-12-69a4ee/1e4c8548-5aa8-429f-9d17-693211694c40/scratchpad`.

Reproduction note: the beautified copies are plain `esbuild.transform(src, {minify:false, format:'esm', legalComments:'none'})` output of the original bundles (script `tmp/cxib/beaut.js`, which uses the esbuild-wasm package from the Cursor install, read-only). Line numbers will match only for that transform. Rating note: `quality` is maturity of execution within this facet; `uniqueness` is relative to what I know of agent browsers in general plus the Verevon, Firecrawl and Apify baselines read for this audit, not a measured comparison against Aside or Cursor.

### REA tools actually used (honest log)
| Tool | Use | Result |
|---|---|---|
| `analyze_javascript_application` | One run (format directory, detail summary, record-and-continue, 2048 MB heap) on a copy of the plugin scripts plus `@oai/sky` Windows/mac modules, `@oai/cua` tinysky-alt and `@oai/cua-repl` (27 JS files). | 27 files parsed, 0 parse failures, 345,458 AST nodes, 322 findings; semantic analysis of `browser-service.mjs` and `browser-client.mjs` exhausted the worker heap, so the graph is partial (evidence `ev_4066e93e8a09c5df20aa8a66f124aac71bc9012216747d4033ce861ca8896e5a`, summary view `ev_7ba590d4fe68ed8d2e560d8aa696c31d8970a3023338f1d03bdb8834668ecd29`). |
| `inspect_analysis_view` | Summary, a modules page and one module item. | Useful only to confirm inventory; the item view added nothing beyond an unresolved `node:path` import. Not pursued further. |
| `trace_application_feature` | Three seeds (string `x-oai-cua-request-budget-ms`, api `spawn`, string `Escape`). | All three returned `no-match` although the strings exist in the sources. This is a REA coverage limit (partial semantic graph), not evidence of absence. |
| `inspect_pe_resources` | `codex-computer-use.exe`, `codex-command-runner.exe`, `codex-windows-sandbox-setup.exe`. | REA-observed: the helper and the command runner have no resources and no embedded manifest; the setup exe has one 358-byte RT_MANIFEST with `requestedExecutionLevel asInvoker`, `uiAccess=false` (evidence `ev_ab4ac839...`, `ev_380f3cc9...`, `ev_41673289...`). Recorded in R105. |
| `open_binary` + `search_strings` | Tried on `codex-computer-use.exe`. | Direct open under `WindowsApps` failed Ghidra admission (Win32 access denied). A scratch copy opened, but two `search_strings` calls timed out at 330 s. I closed the session and used my own printable-string extraction (SRC) for native binaries instead. |

Native binaries were therefore analysed by string extraction only. Behaviour described for native code is inferred from strings and flagged INF or `medium` where needed.

### Limits
- Presence of code is not proof of default enablement or runtime reachability. Several features are flag-gated (TinySky AX mode, guardian v2, raw CDP, audio, computer history).
- The reviewer (guardian) prompts, the site-status service, the cloud broker and the Chrome extension are server-side or outside this package. Only their client contracts are visible.
- No app user-data stores were read. No code was run.

## 1. Executive summary

Codex's browser and computer-use stack is very developed on the *safety and approval* dimension (I did not benchmark it against other audits' sources in this pass, so no cross-product ranking is claimed). The distinguishing facts:

1. **One choke point, many ordered gates.** Every command that touches a page passes `ensureCommandAllowed`, which chains: host URL hook, scheme policy, server-side site-status reputation, enterprise origin policy, saved user denials, then consent (user or an automated reviewer). Outcomes are typed (16 denial reasons with `decisionSource` and `retryable`) and audited with a versioned record (R001, R002, R027).
2. **Policy at the engine, not only in the planner.** While an agent controls a tab the Electron host cancels disallowed navigations itself (redirects, subframes, popups), and the service pauses every main-document response through CDP `Fetch` to vet redirects and downloads (R016, R017).
3. **An independent reviewer ("guardian", UI name "Approve for me") can answer approvals**, with per-(thread, origin) 5-minute approval caching, coalesced reviews, revocation, strict per-call review for WebMCP tools, and a fenced evidence format that treats everything except user-authored text as untrusted (R008-R011, R108).
4. **Credentials never reach the model.** `browserAuth` collects secrets in a host-rendered form, fills them in an isolated CDP world, returns only a status, and then *locks observation of that document* until the agent navigates to a new one; a typed recovery protocol tells the agent what to do next. Email OTP codes can be delivered from a connected mailbox without the model seeing them (R036-R041, R132).
5. **Raw CDP is deny-by-default with a method allowlist** and URL-authorising parameter checks; the safe high-level APIs are what the agent gets (R018, R019).
6. **Policy is also enforced as text, with a read receipt**: the agent must have read the confirmation policy before it may call raw CDP or WebMCP (R029), denials end with an anti-circumvention instruction (R003), and docs are assembled per backend and capability (R030, R031).
7. **Computer use (native Windows)** is window-scoped, observation-bound (indexes, screenshot ids and coordinates are valid for one observation), approval negotiated by the native helper, killed by a *physical* Escape key per turn, and refuses password managers and security software by built-in lists (R079-R089).
8. **The control loop is moving to accessibility-tree indexes with diffed observations ("TinySky")**, replacing coordinate and DOM-node CUA, and the same runtime is used in RL training (R073, R093).
9. **No stealth, no proxies, no robots.txt logic.** The anti-bot posture is the opposite of Firecrawl/Apify: report and stop, never evade (R096, R097). There is no browser record/replay either.

Weaknesses worth knowing (section 9): the site-status gate fails open in the service path while the native helper fails closed; much of the protection is prompt-level text backed by hard gates only in some paths; the reviewer and reputation services are opaque server dependencies and the reputation request carries the full URL path *and query*; TinySky is mid-migration, so two input models ship; the plaintext local TOML mirror and server sync make approvals account-bound, which conflicts with ZDR-style postures unless disabled.

For Verevon the highest-leverage, plane-compatible adoptions are (details in section 10): typed denial/outcome contracts, a per-document credential lock plus vault fill, an authorisation audit record, response-stage redirect gating and URL-bound download grants in Quarry, grant scopes and deny-first precedence in BrowserBroker, and a reviewer sub-agent plus policy text plus policy-ack enforcement in the Model Plane. Nothing here requires giving Quarry approval authority.

## 2. Architecture map

```
Model (Responses API)
  |  tools: exec ("code mode": JS in a V8 isolate, nested tools)   [R077]
  v
Codex core (Rust, codex.exe)  <--- approvals reviewer: user | auto_review | guardian_subagent  [R008, R009]
  |  MCP: node_repl (js, js_reset, turn_ended)  or  cua_repl (unified)             [R075, R078]
  v
node_repl (Node 24.21.0-cua.1 REPL, persistent state)
  |-- browser-client.mjs (agent-facing SDK: agent.browsers.*, tab.ax/playwright/cua/dom_cua, capabilities)
  |-- browser-service.mjs (POLICY + CDP engine)  <-- this is where the gates live      [R001...]
  |       backends: iab | extension | cdp | mcpapps                                    [R139]
  |-- @oai/sky (computer use SDK) --NDJSON stdio--> codex-computer-use.exe (Rust)      [R080]
  v
Desktop host (Electron main process, "owl" Chromium runtime)
  |-- IAB API (class HOe): tabs, debugger leases, downloads grants, navigation restriction, cursor  [R016, R022, R140]
  |-- approvals/preferences: browser/config.toml <-> /wham/browser/settings (revision sync)       [R026]
  |-- node_repl config injected via app-server config/batchWrite                                   [R078]
  v
Chrome extension host (native messaging) / cloud CDP browser / MCP app webviews                    [R066, R139]

Separate, adjacent: command sandbox (restricted token + job + private desktop + firewall-scoped users)  [R099-R105]
```

Trust boundaries that matter: model-authored JavaScript runs inside node_repl (so inputs to gates are copied to frozen data, R089); the service verifies a signed, nonce-bound "local browser use is enabled" proof per turn (R028); the host, not the model, writes the REPL's config and clears shell-level overrides (R078); the native helper owns app policy and asks the host to approve (R081).

## 3. The authorization stack (core of the safety design)

### 3.1 Ordered gate chain per command (R001)
1. Classify the command (`WV`): exempt lifecycle commands (list/get/create/close tab, claim tab, mark, name session, wait) skip checks; navigation checks the *target* URL; everything else checks the *current tab URL* re-read from the tab list.
2. Host hook `runtime.assertBrowserUrlAllowed`.
3. Scheme policy (R015): only `http:`, `https:` and `about:blank`.
4. Site-status reputation (R014): `GET chatgpt.com/backend-api/aura/site_status?site_url=...&url_request_source=codex_browser_use[:backend]&conversation_id&turn_id`; `feature_status.agent === true` means blocked; hostname-keyed (minus `www.`) 24 h cache; localhost exempt.
5. Enterprise origin policy (R006): `requirements.browserUse.origins` merged with user config, deny wins; `access: deny` forces uploads, downloads, raw CDP and auto-review to deny. The generic network permission profile (domain allow/deny, `managedAllowedDomainsOnly`) is folded in too, so one egress policy governs both shell commands and the browser.
6. Saved user decisions and caches (R005), resolved **deny-first**: conversation deny, global deny, enterprise deny, then approvals from narrowest to widest (turn caches, guardian cache, conversation allow, global allow, global never-ask), else prompt.
7. Consent (R004): MCP elicitation `Allow <browser> to access <origin>?` with `persist` options and meta describing risk; answered by the user or by the guardian reviewer (R008). Fail closed when no decision can be obtained (R012).

Each step produces either a pass or a typed `BrowserUseSecurityError {reason, decisionSource, retryable}` (R002) and an audit record `{schema_version:1, permission, decision, decision_context, decided_by, policy, reason, origin, tool_name, backend, duration_ms}` (R027). URL-policy and site-status *passes* are not recorded; only denials.

### 3.2 Specialised gates built on the same pattern
- Files: uploads only through the file-chooser event (R021); downloads need an armed `waitForEvent('download')`, a security check, and a one-time URL-bound grant consumed by the host `will-download` handler; blob downloads are origin-bound (R022).
- Page assets: cross-origin asset fetches need their own approval (R023).
- History: approval, bounded queries, org-level `disabled` (R024).
- Raw CDP: three gates plus method policy (R018, R019, R020).
- WebMCP: per-origin settings, strict reviewer per invocation with the tool input and a sanitised page URL, re-check of tab URL after the approval wait (R010, R043).
- Persistence: "never ask" is available but admins can forbid global persistence; a UI answer of "always" is rejected or downgraded when not allowed (R025).

### 3.3 Reviewer ("guardian")
- Two modes. v1: one origin per session, 5-minute turn cache. v2: cache keyed `{threadId, origin}`, 5-minute expiry, shared pending review promise (concurrent requests coalesce), revocation on decline, and a retry loop that re-validates the cache after each decision (R008). [SRC, high]
- Rust core configuration `GuardianV2ConfigToml` shows a token-budgeted transcript view (tool calls, outputs, reasoning, optionally images), thresholds, lag limits and persisted scores (R009). Evidence-fencing strings state that assistant messages are untrusted context, truncated descriptions do not authorise anything, and missing evidence is never treated as permission (R108). [SRC, medium: strings only]
- Credential prompts are **guardian-incompatible** (`codex_guardian_compatible:false`, `codex_requires_user_input:true`): a human must answer. A separate *precheck* reviewer decides whether the credential prompt may be shown at all, comparing top-level origin, credential frame origin and form submission origin (R011, R036).

### 3.4 Modes and configuration
- Security modes: default; `disabled-for-local-testing` (bypasses URL and reputation gates and most consents); `gaas-browser-environment` (bypasses nothing, only history reads, downloads and asset fetches need no user consent) (R013).
- Preferences live in `CODEX_HOME/browser/config.toml` (0600, atomic write) and are synchronised with `/wham/browser/settings` using `expected_revision`, 409 re-read, polling plus file watch, and principal-change clearing (R026). Defaults are conservative: everything `always_ask`, auto-review on, full CDP off, WebMCP on.
- Signed capability check: the service refuses to act unless the host proves per turn (nonce, session, turn, executor id) that local browser use is enabled (R028).

## 4. Agent control protocol (categories 3 and 9)

### 4.1 Surfaces and input models
- **API shape**: `agent.browsers.{list,get,getDefault,getForUrl}` -> `Browser` -> `Tabs`/`Tab` with `ax`, `playwright`, `cua`, `dom_cua`, `capabilities`, `dev`, `clipboard`, `content`. A per-member `unsupportedByDefaultIn` matrix plus host overrides decides what exists per backend (R051), and documentation is generated from that matrix (R030).
- **Backends**: in-app Electron (debugger API + lease per webContents), Chrome/Edge via extension, remote CDP browser (cloud/orbit/training), and DOM-only MCP app webviews (R139, R140).
- **TinySky switch** (R073): when on, `Tab.ax` is enabled and `Tab.cua`/`Tab.dom_cua` are disabled. Default off for IAB, on for cloud/orbit, on for non-production extension builds. The accessibility path emits a numbered tree with a header (`Browser tab`, `Title`, `URL`, optional `Site Specific Instructions`) and by default only a *diff* from the previous tree (R053, R032). Actions use element indexes; the docs forbid guessing secondary-action names and say to re-read state after each batch.
- **Other paths**: Playwright-compatible locators (via an injected Playwright script on the gated CDP channel, R052), `get_visible_dom` (allowlisted tags and attributes, 200 elements and 20,000 characters, R134), coordinate CUA.
- **Observation channels** are explicit: `ax.write`/`nodeRepl.write`/`emitImage` show things to the model; `ax.get` and friends return data to the script silently (R054).

### 4.2 Robustness machinery
- Stale protection keyed on `documentKey = (mainFrameId, loaderId)` and an AX revision; captures that straddle a navigation are dropped; stale ids raise explicit errors (R055).
- Click safety: content quads, hit-test per frame hop including out-of-process iframes, scroll into view, wait until two consecutive samples agree, else "covered by <tag>" (R056).
- Settling: commit/load 5 s, first paint 5 s, AX quiet 50-100 ms with a 750 ms cap (R058).
- AX capture merges main frame, iframes and OOPIFs with a traversal deadline and per-frame warnings (R057). JS dialogs appear as AX nodes with accept/dismiss/prompt controls; screenshots are refused with guidance while a dialog is open (R059).
- Screenshots: JPEG q80, CSS or device pixels, screencast-based capture that ignores frames older than the request, wrapped by the credential observation gate (R061).
- One retry when the injected execution context is destroyed (R136).

### 4.3 Efficiency rules (DOC, R070)
One focused direct URL attempt, then the site's own search; never `goto` the current URL; prefer the cheapest state check; trust one authoritative on-page signal; proof-of-work screenshot after state-changing actions.

## 5. Credential and secret handling

| Mechanism | What it guarantees | Records |
|---|---|---|
| `browserAuth.request` | The model supplies only non-secret metadata and selectors; the user types secrets in a secure host form; result is a status enum; unavailable means stop, never fall back to chat | R036, R037 |
| Native delivery in an isolated CDP world | Page scripts and the model never receive values; raw CDP is refused and CDP events are not recorded while credential protection is active | R038 |
| Document observation lock | After secrets were delivered, AX, DOM, screenshots and retained snapshots of that document are refused until a new document loads; unknown state poisons the process (fail closed) | R039 |
| Recovery protocol | Typed reasons and `next_action` hints plus a host-side meta record; underlying errors withheld | R040 |
| Reviewer precheck | A reviewer compares three origins before the prompt is shown | R011, R135 |
| Email OTP | Code fetched from a connected mailbox and filled natively after approval; model never sees it | R132 |
| Lifecycle docs | Method selection, QR decoding by the runtime, number-matching read-out, "unknown is not failure", fresh-tab verification | R041 |
| Broker | Hosted sessions use a local-socket credential broker with versioned bindings | R042 |

These are the strongest transferable ideas for Verevon, which currently has **no credential vault or secret-reference action** (baseline known gap).

## 6. Prompt-injection posture

Codex relies on layered controls rather than on detection:
- **Provenance rules** in prompts: user-authored text is intent; pasted, uploaded, website and tool content is never permission; reading is distinguished from transmitting; typing sensitive data is transmission (R033, R034).
- **Hard gates** that do not depend on model obedience: engine-level navigation restriction, response gating, per-origin consent, strict review for WebMCP calls, observation lock (R016, R017, R010, R039).
- **Evidence fencing** for reviewers (R108) and **reviewer-only page capture** so the reviewer's evidence is independent of the planner's snapshot (R135).
- **Documentation read receipts** before dangerous APIs (R029), **anti-circumvention** denial text (R003), and a **server-pushed policy channel**: the confirmation policy text can be supplied per request in metadata and overrides the packaged markdown (R141).
- **Single control channel**: no external MCP browsers, no standalone Playwright, no shell inspection (R123); explicit browser choice is a hard constraint and ambient UI context is not an instruction (R124); session stores are never inspected (R125).
- **Memory hygiene**: an option to keep threads that used MCP or web search out of memory generation (R109, medium).
- There is **no content-level injection detector** in the browser path (no classifier on page text found in either bundle; INF from absence in grep).

## 7. Native computer use (Windows)

- **Window-scoped API** (`list_apps`, `list_windows`, `get_window_state`, `click`, `type_text`, `press_key`, `scroll`, `drag`, `set_value`, `perform_secondary_action`, `activate_window`, `launch_app`); exactly one target window must be selected from returned objects before any input; observe and act are separate steps; every index and coordinate is bound to one observation (R079, R086).
- **Helper protocol**: SDK spawns `codex-computer-use.exe`, NDJSON over stdio, per-request time budget and turn metadata, kill-and-respawn on timeout, `end_turn` on turn change (R080).
- **Approvals**: the *helper* decides which apps need approval and returns `approvalRequest {app, displayName, riskLevel, allowPersistentApproval}`; the SDK asks via elicitation and retries with an approved-app marker; audio is a separate high-risk, session-only class (R081, R088).
- **Physical Escape kill switch**: low-level keyboard hook, exit code 130, marker file per `(conversation, turn)`, all later calls refused with an instruction to stop; overlay says "Esc to cancel" (R082, R065).
- **Built-in refusals**: password managers and security software (R083); non-overridable denies in docs (R035); managed app allow/deny with launch-time identity binding (absolute resolved exe, registered signed package, identity must match; R084).
- **Browser URL policy for desktop-driven browsers**: ends the turn if the URL is disallowed, unverifiable, undeterminable or unsupported (fail closed) (R085).
- **Capture**: Windows.Graphics.Capture (works when occluded), bounded multi-image states with z-order, screenshots emitted at `detail: original` (R087, R092).
- **Approval-input hardening**: because the model writes the JavaScript, approval inputs are checked to be plain data properties and copied to frozen objects before approval and use (R089).
- **Platform variants**: macOS app-scoped, Windows window-scoped, Linux full-desktop with background input that never activates the window (R091).

## 8. Command sandbox and egress (adjacent, relevant to agent sandboxes)

These are for shell commands, not for the browser, but they shape the egress model and are a reference for Model Plane sandboxes (R099-R106):
- Elevated backend creates dedicated local users (offline and online) in `CodexSandboxUsers`, installs per-user firewall rules (block non-loopback, block loopback except the proxy port), stores the offline password DPAPI-protected.
- `codex-command-runner.exe`: restricted token with capability SIDs, job object, private desktop, ConPTY; message protocol `spawn_request -> spawn_ready/output/exit/error`.
- Filesystem profile of read/write/deny entries (paths, globs, special roots) enforced with ACLs, deny-read ACL state file, reparse-point guards; unsupported requests (full-disk write) are refused.
- Managed network: domain allow/deny, SID-bound proxy routing, optional `header_injections`, unix-socket policy; the same domain policy feeds browser origin decisions.
- Exec policy: prefix rules with `Allow | Prompt | Forbidden`; approval policies `on-request`, `on-failure`, `never`, `granular` (the old `untrusted` is rejected) (R106).

## 9. Strengths, weaknesses, limitations

### Best in class
- Typed, audited, fail-closed decision pipeline with explicit decision sources (R001, R002, R027).
- Credential isolation: delivery, observation lock, recovery protocol, reviewer precheck, email OTP (R036-R041, R132).
- Engine-level enforcement (navigation restriction, response gating) and URL-bound single-use download grants (R016, R017, R022).
- Hardware-level user stop and observation-bound input in computer use (R082, R086).
- Capability-conditional docs with read receipts (R029, R030).

### Weaknesses and gaps (evidence-based)
1. **Fail-open vs fail-closed inconsistency**: in the Node service, an error while fetching site-status is swallowed and the request proceeds (`catch { return }`, R014), although `site_status_unavailable` is defined in the catalog; the native helper's URL policy fails closed (R085). [SRC, high]
2. **Reputation request privacy**: the site-status URL includes origin, path and *query string* plus conversation and turn ids (R014). A ZDR-minded design should send hostname only. [SRC, high]
3. **Opaque server dependencies**: reviewer prompts and decisions, site-status, cloud broker and extension are not inspectable; only client contracts are visible. [INF]
4. **Prompt-level controls**: anti-circumvention text, doc-read receipts and many safety rules rely on model compliance; only parts have hard enforcement. `Playwright evaluate` is documented as read-only scope; I did not verify technical enforcement. [DOC/INF, medium]
5. **Two input models in flight**: TinySky AX mode and legacy CUA/DOM-CUA coexist and are mutually exclusive per backend (R073); IAB defaults to the legacy path in this build. [SRC]
6. **No stealth/proxy/robots/record-replay**: by design for stealth, but acquisition features (crawl, extract, robots, proxies) are absent; page fetching is interactive only (`tabs.content` explicitly unsupported in the IAB, R111). [SRC, grep-based absence]
7. **Account-bound approvals**: preferences sync to the account and are mirrored in plaintext TOML; there is no ZDR-style ephemeral mode visible (R026). [SRC]
8. **Computer-use enforcement depth unknown**: forbidden-app lists and managed policy are visible as strings; precise matching and the Escape hook's resistance to injected keys are inferred (R083, R082). [INF]
9. **Telemetry**: product analytics for approvals and tool calls (Statsig) with thread/turn/model identity; switchable off by environment but on by default (R090, R094). [SRC]

### What I could not verify
Guardian reviewer prompt text; whether `file:` URLs are navigable in practice (service policy blocks them while the plugin description mentions them, R015); how `accessibilityCore` (WASM) renders diffs; the Chrome extension's request header; cloud browser infrastructure (egress, proxies).

## 10. What Verevon should copy, adapt or avoid

Constraints from the Verevon baselines (not re-verified): Model Plane plans and approves, Quarry executes or rejects and **must not gain approval-decision authority**; BrowserBroker owns domain authority and revalidates grants before every action; one-shot approval grants already exist for dialog/upload/download; refs are `@eN` with verify-and-act; ZDR must propagate (no durable receipts or profiles); the browser UI never sees CDP endpoints; there is no credential vault, no prompt-injection labelling, `evaluate` and raw-CSS actions need no approval, remote providers are blocked for agent runs until isolated egress is proven.

### P0 (small, high value, plane-compatible)
| # | Adopt | Where | Source records |
|---|---|---|---|
| 1 | Typed denial and outcome contract (`reason`, `decisionSource`, `retryable`, `next_action`) on every Quarry refusal; anti-circumvention suffix on model-visible denials | Quarry observation `policy_denials`; Model Plane prompt | R002, R003, R040 |
| 2 | Per-document credential lock plus vault-fill action returning status only; isolated-world fill; suppress console/network capture during fill; invalidate `@eN` refs and snapshots until a new document loads | Quarry driver + new action; Model Plane planner | R038, R039, R036 |
| 3 | Authorisation audit record (`decided_by`, `decision_context`, origin-only) next to egress receipts; none for ZDR orgs | Quarry receipts + Application Plane | R027 |
| 4 | Provenance and injection policy text, evidence fencing for any reviewer, default-deny on truncated evidence | Model Plane system prompts | R033, R034, R108 |
| 5 | Response-stage Document gate so redirects and downloads are vetted before commit; fail-closed interceptors; engine-level popup/subframe blocking | Quarry chromiumoxide driver | R016, R017 |
| 6 | URL-bound, single-use, expiring download grant; blob origin binding (turns the driver-gated `download_ref` on safely) | Quarry + BrowserBroker grant | R022 |
| 7 | Gate `evaluate`/raw CSS behind the same grant path and make `evaluate` read-only by construction | Quarry + Model Plane | R052, R018 |

### P1
| # | Adopt | Where | Source records |
|---|---|---|---|
| 8 | Grant scopes (turn, conversation, persistent), deny-first precedence ladder, org switch for persistence, 3-form origin patterns (`exact`, `*.`, `**.`) | BrowserBroker | R004, R005, R007, R025 |
| 9 | Reviewer sub-agent in execution-core with a short-lived per-(thread, origin) approval cache, coalesced pending reviews, revocation, token-capped transcript sources, optional image evidence, reviewer-only evidence capture from Quarry | Model Plane | R008, R009, R135 |
| 10 | Policy-ack token: dangerous Quarry actions refuse unless the Model Plane session proves the policy was loaded; re-inject docs after compaction | Model Plane + Quarry | R029, R076 |
| 11 | Capability matrix per driver and generated prompts/docs; capability registry with env kill switches | Quarry drivers + Model Plane | R030, R050, R051 |
| 12 | Per-step time budget and turn id on every Quarry call; retry-once rules; settle budgets | Quarry step contract | R095, R080, R058 |
| 13 | Reputation/denylist lookup before navigation with explicit fail-open/closed choice, hostname-only requests | Quarry control (blocklists exist) | R014 |
| 14 | Observation lock and stale guards: loader-id keyed capture discard, covered-element errors, window/frame binding for coordinate actions | Quarry snapshot/actions | R055, R056, R086 |
| 15 | AX-diff observations to cut planner tokens; hard budgets on DOM summaries (200 elements, 20k chars) | Quarry observation | R053, R134 |
| 16 | User stop that invalidates the whole run, not only the next step; persistent "agent is controlling" indicator | Application Plane + live view | R082, R065 |

### P2 (design later)
- Managed network proxy with domain policy and server-side credential/header injection for governed egress (R103); connector-to-browser bridge for OTP and token delivery via integration-corev2 (R132, R112); per-domain site playbooks fed to the planner (R032); code-mode style scripts only if the script runner stays in Model Plane and calls Quarry per action (R077); native-app computer use with forbidden lists and identity binding (R083, R084); signed tool bundles (R117); ambient-capture with per-app/domain observe policy and clear-history, never under ZDR (R133).

### Avoid
- Putting approval and risk classification inside the browser layer (Codex colocates them in the host; Verevon's plane rule forbids it for Quarry).
- Fail-open reputation checks, full URLs with query strings sent to third parties, plaintext local copies of org policy, and defaults that persist approvals beyond what org policy allows.
- Exposing raw CDP or `document.modelContext`-style tools without the per-method policy and per-call review (R018, R010).
- Letting stealth dead code and 'captcha solving' mixed signals persist; adopt the explicit no-evasion stance (R096) and the closed bot-block reason set (R097).

### Quarry/BrowserBroker feature-parity view for categories 3, 4, 9
- **Category 3 (rendering/engines)**: Codex has no stealth, fingerprinting, geolocation/locale emulation or proxy handling to copy (verified absent). What is worth copying is engine-level control: debugger leases, flat target sessions, hidden-page throttling exemptions, screencast freshness, isolated-world injection, OOPIF-aware AX/DOM capture (R057, R061, R129, R139, R140).
- **Category 4 (anti-bot, reliability)**: copy the closed report taxonomy, hostname-only recording, one-recovery-attempt rule, layered timeouts, retry-once-on-destroyed-context, structured retryability, and the idea of an agent-identifying request header (R095-R097, R136, R137). Do **not** expect proxy pools, CAPTCHA services or robots handling from Codex; those stay a Quarry/Firecrawl/Apify matter.
- **Category 9 (AI-browser control)**: Verevon already has the right skeleton (refs, grants, one-shot approvals, takeover, receipts). The gaps Codex fills are the credential lifecycle, reviewer-based approvals, typed denials, reputation gate, observation-bound input, AX diffing, doc-read enforcement and per-document locks.

## 11. Corrections to the scout inventory
- Inventory said the restricted-navigation scheme list is about/blob/data/http/https. True for the *main-process* restriction (R016), but the *plugin service* policy admits only http, https and about:blank (R015).
- Inventory listed `Fetch` CDP as "only disable present". The service uses `Fetch.enable` with a Response-stage Document pattern on every tab (R017); only the *raw CDP* path forbids `Fetch.disable` and non-Document patterns it cannot reconcile.
- Inventory marked "robots.txt, stealth, proxy: not found". Confirmed by grep in both bundles (zero matches for `setUserAgent`, `UserAgentOverride`, `stealth`, `navigator.webdriver`, `setGeolocationOverride`, `setTimezoneOverride`, `setLocaleOverride`, `setProxy`, `robots.txt`) (R096).
- Inventory said `unified-computer-use` is disabled by default; confirmed (`enabled:false` in `.mcp.json`), and it is gated by the TinySky desktop feature (R073, R075).
- New facts not in the inventory: the guardian/auto-review system, the reputation service, credential observation lock, email OTP flow, WebMCP strict review, per-environment doc sets, doc-read enforcement, Windows helper protocol and Escape kill switch, managed app policy, command sandbox architecture, settings sync.

## 12. Open questions
1. Exact reviewer prompts, risk scoring and thresholds (server-side).
2. Whether the IAB default will flip to TinySky AX mode in a later build, and whether coordinate CUA will be removed.
3. How strictly `evaluate` is sandboxed (read-only enforcement in the injected runtime).
4. Whether `file:` navigation is permitted in practice for local development in the IAB.
5. The extension's agent request header name and format, and whether it is meant as a bot-signal for sites.
6. Whether the guardian cache scope/TTL differs under `gaas-browser-environment`.

## Appendix A. Record index by category

(Generated from the JSON; id suffix, quality 1-5, confidence, name.)

### AI-browser control (78)

| id | q | conf | name |
|---|---|---|---|
| 001 | 5 | high | Per-command security choke point (ensureCommandAllowed) |
| 002 | 5 | high | Typed denial taxonomy with decisionSource and retryable flag |
| 003 | 4 | high | Anti-circumvention instruction appended to denials |
| 004 | 5 | high | Per-origin consent prompt with scoped persistence |
| 005 | 5 | high | Deny-first permission resolution order |
| 006 | 5 | high | Enterprise per-origin policy with deny-wins merge |
| 007 | 4 | high | Origin pattern language with apex and subdomain wildcards |
| 008 | 5 | high | Guardian auto-review of origin access ('Approve for me') |
| 009 | 4 | medium | Guardian v2 reviewer configuration and transcript budgeting |
| 010 | 5 | high | Strict auto-review for every WebMCP tool invocation |
| 011 | 5 | high | Automated safety precheck for credential prompts |
| 012 | 5 | high | Fail-closed approval semantics |
| 013 | 4 | high | Security modes (default, local testing, GaaS) |
| 014 | 4 | high | Server-side site-status (URL reputation) gate |
| 015 | 4 | high | Navigation scheme policy (http/https/about:blank only) |
| 016 | 5 | high | Main-process navigation restriction while the agent controls a tab |
| 017 | 4 | high | Document-response interception with fail-closed request interceptors |
| 018 | 5 | high | Raw CDP behind three gates plus a method allowlist |
| 019 | 5 | high | URL-authorising CDP parameters (cookies, requests) |
| 020 | 4 | high | CDP event buffer with cursors and origin-change purge |
| 021 | 4 | high | Gated file upload through the file-chooser only |
| 022 | 5 | high | Download gating with one-time grants |
| 023 | 4 | high | Cross-origin page-asset fetch approval |
| 024 | 4 | high | History access approval with bounded queries |
| 025 | 4 | high | Persistent approvals gated by enterprise policy |
| 028 | 4 | high | Signed per-turn verification of 'local browser use enabled' |
| 029 | 5 | high | Documentation-read enforcement before risky APIs |
| 030 | 4 | high | Capability-conditional documentation assembly |
| 031 | 4 | high | Per-environment documentation sets |
| 032 | 4 | high | Site-specific instructions injected into page observations |
| 033 | 5 | high | Prompt-injection and authorization policy text |
| 034 | 5 | high | Four-tier confirmation taxonomy |
| 035 | 4 | high | Non-negotiable desktop automation denies |
| 036 | 5 | high | Secure sign-in handoff (browserAuth) |
| 037 | 5 | high | browserAuth request validation (origin, selectors, field heuristics) |
| 038 | 5 | high | Native credential delivery in an isolated world |
| 039 | 5 | high | Credential-protection observation gate (state machine) |
| 040 | 5 | high | Machine-readable credential recovery protocol |
| 041 | 5 | high | Sign-in lifecycle rules (methods, QR, MFA, unknown outcomes) |
| 043 | 4 | high | WebMCP site tools (page-defined agent tools) |
| 045 | 4 | high | User takeover and interruption signalling |
| 046 | 4 | high | Cloud browser manual handoff |
| 047 | 4 | high | Tab claiming and mention snapshots that fail closed |
| 049 | 4 | high | Browser management capability with audit trail |
| 052 | 4 | high | Playwright-compatible semantic locators with read-only evaluate |
| 053 | 5 | high | Accessibility-first control with index targets and tree diffing |
| 054 | 4 | high | Model-visible vs programmatic observation channels |
| 055 | 5 | high | Stale target protection (document key and AX revision) |
| 056 | 5 | high | Hit-tested clicks with scroll, settle and covered-element errors |
| 059 | 4 | high | JavaScript dialogs exposed as controllable AX nodes |
| 060 | 4 | high | Three input paths: AX, DOM-node CUA, coordinate CUA |
| 070 | 4 | high | Efficiency and reliability rules for lookup tasks |
| 071 | 5 | high | Surface selection: connector first, browser only on explicit or fallback |
| 073 | 4 | high | TinySky mode: accessibility-index control replaces coordinate and DOM CUA |
| 074 | 4 | high | Unified cua API over native apps and browsers |
| 076 | 4 | high | First-use documentation emission and post-summary re-emission |
| 077 | 4 | high | Code mode: the model writes JavaScript that orchestrates tools |
| 078 | 4 | high | node_repl host hardening and configuration injection |
| 079 | 4 | high | Windows window-scoped computer use (window2 API) |
| 080 | 4 | high | Native helper protocol with turn scoping and request budgets |
| 081 | 5 | high | Per-app approval negotiated by the native helper |
| 082 | 5 | high | Physical Escape-key kill switch with per-turn interrupt marker |
| 083 | 4 | high | Built-in forbidden application classes |
| 084 | 5 | medium | Managed (enterprise) app policy with launch-time identity binding |
| 085 | 4 | medium | URL policy for browsers driven through desktop automation |
| 086 | 5 | high | Stale window and coordinate-input protection |
| 088 | 3 | high | Computer audio recording with separate approval |
| 089 | 5 | high | Approval-input hardening against getter/proxy tricks |
| 098 | 4 | high | Web search before browser: information lookups avoid page automation |
| 108 | 5 | high | Evidence fencing for reviewers and delegated messages |
| 122 | 4 | high | Elicitation-based approvals usable by any MCP host |
| 123 | 4 | high | Single control channel: no side doors around the gated runtime |
| 124 | 4 | high | Explicit browser choice is a hard constraint; ambient UI context is not an instruction |
| 125 | 4 | high | Read-only discovery: session stores are off limits |
| 130 | 4 | high | Certificate-error interstitials are detected and require hand-off to bypass |
| 131 | 3 | high | Agent-visible safety stance on local environment |
| 135 | 4 | medium | Reviewer-only page context capture |
| 141 | 4 | high | Server-pushed confirmation policy text per request (override of packaged docs) |

### Observability & operations (25)

| id | q | conf | name |
|---|---|---|---|
| 026 | 4 | high | Browser preferences synced to the server with optimistic concurrency |
| 027 | 5 | high | Structured authorization audit record per decision |
| 044 | 3 | medium | WebMCP activity events to the host UI |
| 067 | 3 | high | Response metadata piggyback (tool surface, sanitised URL, tab list) |
| 068 | 3 | high | Browser notifications appended to tool results |
| 069 | 4 | high | Per-command spans with origin-only attributes and user-wait excluded |
| 090 | 3 | high | Computer-use telemetry (approval and tool-call events) |
| 094 | 3 | high | Ambient-network switch for telemetry and feature gates |
| 099 | 5 | high | Windows elevated sandbox: dedicated accounts plus firewall rules |
| 100 | 5 | high | Command runner: restricted token, job object, private desktop, ConPTY |
| 101 | 5 | high | Filesystem permission profile with deny-read ACLs and reparse-point guards |
| 102 | 4 | medium | Capability SIDs per writable root |
| 103 | 5 | high | Managed network proxy with domain policy and header injection |
| 104 | 3 | medium | Sandbox environment hardening and chunked launch payload |
| 105 | 4 | high | Sandbox service, setup elevation and implementation modes |
| 106 | 4 | medium | Exec policy rules (Allow / Prompt / Forbidden) and approval policy values |
| 107 | 4 | medium | Approval outcome taxonomy and review counters |
| 109 | 4 | medium | Memory pollution guard for web and MCP use |
| 115 | 4 | high | MCP app webview sandbox |
| 116 | 5 | high | Embedded webview attach hardening |
| 118 | 4 | medium | Cloud tunnel destination validation |
| 126 | 3 | high | Console log capture for the agent (levels, filter, limit) |
| 127 | 3 | medium | Standardised identity block on agent telemetry |
| 128 | 3 | high | Setup and diagnostics scripts for external browser control |
| 133 | 3 | medium | Computer history recorder with per-app and per-domain observation policy (macOS) |

### Integration-plane overlap (4)

| id | q | conf | name |
|---|---|---|---|
| 042 | 4 | medium | Cloud credential broker bindings |
| 066 | 4 | medium | Chrome extension bridge (native messaging host) |
| 072 | 3 | high | Plugin mention deep links |
| 132 | 5 | high | Email one-time codes delivered to a page without the model seeing them |

### Orchestration & scale (3)

| id | q | conf | name |
|---|---|---|---|
| 048 | 4 | high | Turn-scoped tab lifecycle with marks |
| 120 | 4 | high | Per-conversation browser state with admission backstops |
| 138 | 4 | high | Sub-agent aware browser sessions and turn tracking |

### Platform & ecosystem (6)

| id | q | conf | name |
|---|---|---|---|
| 050 | 4 | high | Capability registry with documentation-on-demand |
| 051 | 4 | high | Backend matrix via unsupportedByDefaultIn |
| 075 | 4 | high | cua_repl MCP server packaging |
| 093 | 4 | high | Training parity: same runtime for RL training and production |
| 117 | 4 | high | Signed bundled plugins with CI provenance |
| 119 | 4 | high | Feature gating through config keys and desktop availability |

### Rendering & browser engines (9)

| id | q | conf | name |
|---|---|---|---|
| 057 | 4 | high | AX capture across frames and processes with deadlines |
| 058 | 4 | high | Settle heuristics after actions |
| 061 | 4 | high | Screenshot pipeline (JPEG, css/device pixels, fresh screencast frames) |
| 062 | 3 | medium | Virtual clipboard per browser session with restore |
| 087 | 4 | high | Occlusion-proof window capture with bounded multi-image states |
| 091 | 4 | high | Platform-specific desktop APIs with the same model |
| 129 | 4 | high | Background pages keep full speed while agent-driven or captured |
| 139 | 4 | high | Four browser backends behind one service |
| 140 | 4 | high | Debugger leases and flat target sessions in the in-app browser |

### In-app browser shell (user-facing) (5)

| id | q | conf | name |
|---|---|---|---|
| 063 | 4 | high | Agent cursor with arrival wait |
| 064 | 5 | high | Approval prompt contract (risk level, persistence options, display params) |
| 065 | 4 | high | 'Using your computer / Esc to cancel' overlay |
| 114 | 3 | high | Site-tools settings deep links in denial messages |
| 121 | 3 | high | Request-specific viewport and visibility control |

### Output formats & content processing (4)

| id | q | conf | name |
|---|---|---|---|
| 092 | 4 | medium | Image preparation: detail levels, resize notices and a unified image budget |
| 112 | 4 | high | Google Workspace export and YouTube transcript through the user's session |
| 113 | 4 | high | Page asset inventory and bundling |
| 134 | 4 | high | Visible-DOM projection with tag/attribute allowlist and hard budgets |

### Anti-bot, proxies & reliability (5)

| id | q | conf | name |
|---|---|---|---|
| 095 | 4 | high | Layered timeouts and retry guidance |
| 096 | 4 | high | No stealth: explicit anti-evasion policy for bot blocks |
| 097 | 4 | high | Bot-detection reporting capability (hostname-only telemetry) |
| 136 | 3 | high | One automatic retry when the isolated execution context is destroyed |
| 137 | 3 | medium | Agent-identifying request header on the extension backend (gate-controlled) |

### Acquisition API surface (2)

| id | q | conf | name |
|---|---|---|---|
| 110 | 3 | medium | Hosted web_search tool configuration |
| 111 | 2 | high | No batch content API in the in-app browser |

