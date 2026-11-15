# Codex desktop in-app browser and browser-use: deep dive (Audit 2, facet in-app-browser)

Scope: how the Codex (OpenAI.Codex 26.1007.2314.0, app 26.1007.21434) in-app browser, its agent-control stack and its safety model work, and what Verevon and Quarry v2 should copy or avoid. Read-only static analysis. Nothing was run.

Feature records: `codexbrowser.in-app-browser.features.json` (120 records, ids `codexbrowser-in-app-browser-001..120`). In this document `F###` means the record with that three-digit suffix.

## 0. Method, provenance and limits

Provenance tags used throughout: SRC = read in shipped code (bundles, plugin scripts, manifests, renderer assets); DOC = packaged documentation (plugin docs, api.json, SKILL.md); REA = observed through an REA tool; INF = my inference (basis stated).

Path legend (all read-only):
- EXTRACT = `scratchpad/extract/codex/` (asar copy). Main process `.vite/build/main-BklS_2Y2.js` (3.8 MB minified), page preload `.vite/build/browser-page-preload.js`, shared constants `.vite/build/src-CP7XXlpZ.js`, `bootstrap-487kJHw_.js`. Renderer `webview/assets/` (app-shared and app-initial bundles hold the i18n catalog).
- INSTALL = `C:/Program Files/WindowsApps/OpenAI.Codex_26.1007.2314.0_x64__2p2nqsd0c76g0/app`. PLUG = `INSTALL/resources/plugins/openai-bundled/plugins` (browser, chrome, computer-use, unified-computer-use).
- Pretty copies: every minified bundle was reformatted with esbuild-wasm (names preserved, no minification) into `scratchpad/tmp/cxib/*.pretty.js`. Line numbers in the records refer to these copies (`main.pretty.js`, `preload.pretty.js`, `browser-service.pretty.js`, `browser-client.pretty.js`, `bootstrap.pretty.js`, `src-CP7.pretty.js`). Records name the original file next to each reference.

REA tools actually used (honest log):
- `analyze_javascript_application` (format directory, summary, record-and-continue, 2048 MB, 600 s) on `PLUG/browser/scripts`: succeeded. Evidence `ev_3d3e178c...` (parent `ev_32feb7f3...`): 9 JS files parsed, 1,696,069 text bytes, 336,139 AST nodes, 151 findings, 0 parse failures; Electron surfaces all 0 (browser_windows, preload_entrypoints, context_bridge_apis, IPC operations, utility processes); 49 sender/origin-check observations; semantic graph partial because the two big bundles exhausted the worker heap (2,010 semantic unknowns). See F022.
- `inspect_pe_resources` on `Codex.exe`: succeeded. Evidence `ev_70342411...`: 20,272 bytes, one RT_MANIFEST resource, no icons. See F012.
- `trace_application_feature` and `inspect_analysis_view` against the retained evidence: both failed with `evidence_integrity_mismatch` (evidence no longer retained in the shared REA session). No claim here depends on them.
- The scout had already run one heavy analysis on `.vite/build`; I did not repeat it. Main-process analysis here is by reading the reformatted bundles, so the main-process records are SRC, not REA.

Limits: no runtime observation, no user-data stores read (no cookies, history, passwords, `browser/config.toml`, Local Storage), no secrets seen. The browser implementation source (`lib/oai_browser_use`) is not shipped; everything is from compiled bundles. A gate present in code is not proof it is enabled in the audited build (most gates default to false, F020).

## 1. Executive summary

1. The in-app browser is a real Chromium 155 browser (project name "owl"), not a thin webview. The install ships `chrome.dll` and Chrome-only services; `Codex.exe` is a 20 KB launcher; the main process talks to a browser-level session object over capnweb RPC (history, top sites, favicons, cookies, profile import, autofill, omnibox) [SRC, REA] (F028, F012). That is why password manager, extensions, site settings and the omnibox are all present.
2. Tabs are Electron `<webview>` guests, one per tab, attached under main-process control: attach params are rewritten, preferences hardened (sandbox, contextIsolation, no node, no popups) and every guest uses one shared persistent profile `persist:codex-browser-app` [SRC] (F029, F030). Tabs belong to a chat task (conversation id + tab id), are suspended past a 32-page budget and restored from snapshots with real navigation history [SRC] (F031, F034, F035).
3. The agent does not get fixed tools. It writes JavaScript in a persistent Node REPL against a typed browser runtime (`browser-client.mjs`) whose roughly 85 commands are schema-validated and shared by four backends: embedded browser, Chrome-family extension, remote CDP, MCP Apps (F075, F076, F009). The runtime is Electron-free plain ESM [REA] (F022).
4. Observation is accessibility-tree-first: indexed AX text with diffs, backed by per-frame CDP AX trees merged by a WASM engine, with DOM snapshot, vision and Playwright locators as fallbacks (F080, F082, F083). Actions are preceded by an animated agent cursor that the UI acknowledges (F085).
5. The safety model is the standout. Every refusal has a typed reason from a 16-value taxonomy and a no-workaround instruction (F091); consent is per origin and per capability (browse, download, upload, raw CDP, site tools, history) with wildcard rules in a synced TOML policy (F093, F056); approvals use MCP elicitation answered by the user or an automated reviewer with a 5-minute origin cache (F092); sites are checked against a server blocklist (F094); navigation is restricted to http/https/about:blank (F095); raw CDP is filtered by domain, method and parameter (F096); every decision is audited (F025).
6. Credentials never reach the model. `browserAuth` fills a secure form natively, binds origin and frame, then blocks model observation (AX, screenshots, CDP diagnostics) until a new document loads (F101, F102, F103). Read-only JS evaluation hides credential fields (F114). The agent clipboard is virtual (F100).
7. WebMCP ("Site tools") lets pages publish tools through `document.modelContext`; the host discovers them per frame over CDP, validates and size-limits descriptors, and gates invocation behind strict auto-review (F097). The Site Annotation API (`document.oai.annotation`) is the user-side counterpart: pages opt in to being commented on, with user review of anything the page contributes (F063, F064).
8. User-facing annotation is a first-class page-context channel: element, text and region comments with a rich target schema, a design editor that turns visual CSS edits into diffs, @tab mentions with fail-closed staleness checks, image drag into the composer (F059 to F062, F066).
9. Weak spots: the Windows named pipe has no peer authorization (macOS only) (F078); the site-status check fails open on request failure and sends URL paths to OpenAI (F094); one shared profile means agent and user share logins (F030); zxing WASM default loader points at a CDN (F008); the policy layer (`browser-service.mjs`) is loaded by the same node_repl host that runs model-written JS (INF: the service wraps `process` with a shim, which suggests an isolated context, but the isolation was not verified), so integrity depends on that boundary plus host-signed answers (F109, F120).
10. Verevon relevance: Quarry already has the primitives Codex built on top of (AX snapshot refs, frame refs, dialogs, one-shot approval grants). What Verevon lacks is the product layer: surfacing refs/AX to the model, a per-origin consent matrix, typed denial reasons, a credential broker with an observation gate, an agent cursor, comment/annotate on page, and a working grant path (section 7).

## 2. Architecture

```
 Renderer (React, webview tag host)            Main process (Electron-compatible, "owl" Chromium 155)
 +-----------------------------+              +-------------------------------------------------------+
 | tab strip, omnibox, panels  |  IPC         | BrowserHostManager (kje)  per-window / per-thread state |
 | hidden-browser-use hosts    |<------------>|  tab budget (rje), snapshots, comments, permissions     |
 | settings (policy matrix)    |              | BrowserSessionRegistry (kq)  session routes, debug ring |
 +--------------+--------------+              | IAB backend (HOe)  CDP via webContents.debugger        |
                | <webview> guests            | native pipe server (lke) \\.\pipe\codex-browser-use     |
                v                             +-------------------+-----------------------------------+
 Guest page (sandboxed, partition                                  | length-prefixed JSON frames
 persist:codex-browser-app)                                        v
   + browser-page-preload.js                          node_repl (model JS)  +  browser-client.mjs
     comment overlay, document.oai,                     +  browser-service.mjs (policy, AX, locators)
     WebMCP toolchange, mouse nav                       browser-accessibility.wasm, Playwright injected script
```

Other backends plug into the same client: Chrome-family extension (`extension-host.exe`, native messaging host `com.openai.codexextension`), remote CDP browser ("Cloud Browser", behind an operator proxy), MCP Apps webviews (F009, F010, F110).

### 2.1 Engine and shell model [SRC]
- Owl runtime named in `resources/owl-electron-app.json`; Chromium 155.0.8059.40 manifest; `elevation_service.exe`, `chrome_pwa_launcher.exe`, `IwaKeyDistribution` present (F028). Electron 42.3.0 is declared in package.json (scout), so owl is a Chromium host exposing Electron-compatible APIs plus extras: `webContents.clone()`, `showPageInfoDialog`, `setPageCapturePaintLeaseEnabled`, `capturePage(...,{stayHidden})`, session `permissionHandling:'browser'` and `setPermissionPromptHandler` (INF that these are custom, SRC that they are called).
- Guest creation: `will-attach-webview` handler (`ICe`, main.pretty L36712) matches the attach to a pre-registered route (conversation, tab, host generation, renderer instance) or calls `preventDefault`. `LCe`/`cU` force partition `persist:codex-browser-app`, the shared session, the preload, `sandbox=true`, `contextIsolation=true`, `nodeIntegration=false` (also in subframes and workers), `webSecurity=true`, `webviewTag=false`, `plugins=false`, `disablePopups=true`, and delete `allowpopups`, `disablewebsecurity`, `webpreferences` attributes. `src` is blanked so the app owns first navigation (F029).
- The renderer-supplied partition string `persist:codex-browser-app-route:<conversation>\0<tab>:host:<renderer>:<gen>` is only a routing token (F030).

### 2.2 Thread, tab and window model [SRC]
- A page is keyed by conversationId + browserTabId. Each task has its own tab set; host kinds right, bottom, hidden-browser-use; detached windows with `tabSourceWindow`, transfers with tokens (F031, F033, F032).
- Tab budget: only live-detached pages count toward 32; protection for active, working-set (30 min), presented, agent-active, tab-capture, audible, camera/mic, loading, restoring, downloading; suspension order inactive-secondary, inactive-selected, current-thread-secondary (F034).
- Snapshot persistence: `browser-sidebar-page-states.json` with Chromium navigation entries and `pageState`; cold restore through `navigationHistory.restore` (F035).
- Agent tabs are created with `createPanelTab:true, active:false`, mounted as hidden webviews, and shown only on request (F068, F087).

### 2.3 Agent protocol [SRC, DOC]
- Bootstrap: skill `control-in-app-browser` instructs `setupBrowserRuntime()`, backend selection, full `documentation()` read; a per-thread `node_repl` MCP server is launched with env that selects backends, disables API members and capabilities, sets security mode and passes pipe paths (F075).
- Commands: `AtlasCommand(type, zodSchema, payload)`; about 85 types (tabs, AX, CUA, DOM-CUA, Playwright, clipboard, content export, dev logs, dialogs, WebMCP, management, viewport, visibility, user tabs, history). The class name suggests the protocol predates this app and also serves the Atlas browser (INF) (F076).
- Capability negotiation: `api.json` marks each member `unsupportedByDefaultIn` per backend; `getInfo` returns `apiSupportOverrides`; capabilities are listed and documented at runtime; `documents.json` includes docs conditionally; `assertRequiredDocumentationRead` blocks CDP, WebMCP and browserAuth commands until the docs were read (F077, F001).
- Transport: named pipe `\\.\pipe\codex-browser-use` (or `/tmp/codex-browser-use`), length-prefixed JSON, 8 MB in and 64 MB out, pushes CDP events, detach events, page events, download changes (F078). Peer authorization is compiled only for macOS (F078).
- IAB backend (`HOe`): per-session tabs, `executeCdp` through `webContents.debugger` with a debugger lease per guest, emulated `Target.getTargetInfo/getTargets/closeTarget` (target id `browser-use-iab-tab:<n>`), refusal of other `Target.*` and `Input.*` beyond mouse, key and insertText, 20 s command timeout, page-ready 15 s (F079, F015).

### 2.4 Observation and action stack [SRC]
- AX text: per frame `Accessibility.getFullAXTree` + DOMSnapshot, merged across cross-origin iframes through `Target.attachToTarget(flatten)` and `DOM.getFrameOwner`; a WASM engine (`computer_use_browser_wasm_revision_*`) creates revisions with text, per-element identity and settable-value flag, and diffs against the previous revision (F080, F084). Output is token-efficient diffs by default.
- Actions: AX index actions (click, drag, paste, type, key, scroll, select text, set value, secondary action, held clicks and keys), DOM-CUA by node id, CUA by coordinates, Playwright locators with embedded InjectedScript and failure diagnostics (F081, F082, F083). Keyboard synthesis models platform modifiers, editing commands, deadlines and input target tokens (F115). Every pointer action first animates the agent cursor and waits up to 1.5 s for the UI ack (F085).
- Read-only evaluate: isolated world `browser-use-readonly-js`, wrapper proxies for DOM objects, credential-field policy, result caps (depth 8, 2,000 array items, 200 keys, 200k chars), `import()` rejected (F114).
- Virtual clipboard shim replaces `navigator.clipboard` in the page (F100). Downloads need URL-bound short grants (10 s) and blob origin binding (F098). Uploads use the file chooser flow (F099). Dialogs are objects with accept/dismiss (F108).
- Network layer: `Fetch.enable` on Document responses is reserved for the host (download detection, site instructions) and cannot be requested by raw CDP (F096, F014).

### 2.5 Safety stack [SRC, DOC]
Order of evaluation for a navigation or sensitive action (reconstructed; exact order is INF):
1. Backend capability and API-member support (`unsupportedByDefaultIn`, env disables) [F077].
2. Navigation URL policy: `Gre()` allows only http, https, about:blank for agent-controlled tabs; enforced in webRequest and navigation handlers; blocked navigations emit `Page.navigationBlocked` [F095].
3. Site status: GET `/aura/site_status` per origin+path, 24 h cache, localhost skipped [F094].
4. Enterprise network policy and persisted user rules (`origins`, `downloads`, `uploads`, `full_cdp`, `webmcp` allowed/denied lists, wildcard `*.` and `**.`), plus approval modes [F093, F057].
5. Consent prompt via MCP elicitation, answered by user or automated reviewer; accepted automated approvals cached 5 minutes per thread and origin [F092].
6. For credentials: broker form, origin and frame binding, automated safety pre-check with screenshot and action targets, observation gate [F101, F102, F104].
7. Audit event for every check with outcome and permission source [F025]; typed error reason to the model [F091].

Policy storage: `<CODEX_HOME>/browser/config.toml` (preferences plus rule tables) written atomically (0600), hot-reloaded, projected to `/wham/browser/settings` with optimistic `expected_revision` (F056). The settings UI is a matrix of Site or pattern x Browsing, Downloads, Uploads, Debug/CDP, Site tools with defaults Allow, Always allow, Requires approval, Block and a Managed by your organization legend (F071, F057). Default full CDP is off, WebMCP default on (when the gate is on), all approval modes default `always_ask`.

Trust boundary insight: model-written code runs in the same REPL host as the browser client (INF), so access answers that matter are produced by the desktop host and signed (Ed25519, domain `codex.browserAccess.v1`) with the public key handed to the REPL out of band (F109, F120). Site instruction headers are only honored behind the operator proxy (F014).

### 2.6 User-facing shell [SRC]
- Chromium features exposed: omnibox with real suggestion engine (F036), password manager, autofill, site settings, extensions manager and toolbar actions (F045 to F048), profile import from Chrome and Atlas with managed-policy off switch (F047), download manager (F042), history with agent filter (F043), clear data dialog (F044), find, zoom, print, mute (F038, F039), media indicators (F040), permission prompts (F041).
- Page context: context menu (Comment, Quick annotate, Attach image) (F049), annotation overlay in the page (F061), structured target schema (F060), design editor (F062), Site Annotation API with permission dialog and user-review rules (F063, F064), server-side site blocklist (F065), image drag (F066), @tab mention (F059).
- Placement: right, bottom, detached windows, floating composer, side chat (F033); cloud browser live view with click-to-take-control overlay (F067); agent PiP (F069, gated).

## 3. Data model summary

| Object | Key fields | Where |
|---|---|---|
| Page/tab state | conversationId, browserTabId, hostGeneration, snapshot (url, title, favicon, zoomPercent, interactionMode, tabType, commentModeDisabledReason), isBrowserUsePage, browserUseTabLifetime (temporary or persistent), isBrowserUseNavigationRestrictionActive, browserStorageId | main (kje), `thread-browser-tabs-v1:` |
| IAB tab record | cdpTabId (int), browserTabId, routeKey, lifetime, sessionControlled, mark{status,turnId}, pageKey, webContents | HOe |
| Session route | sessionId, browserConversationId, windowId, ownerWebContentsId, disposeAfterSessionActivity | kq |
| Snapshot entry | runtime electron-webview, navigation{currentIndex, entries[{url,title,pageState}]}, title, faviconUrl, updatedAt | browser-sidebar-page-states.json |
| Policy file | preferences (approval_mode, history_approval_mode, iab_history_approval_mode, download_approval_mode, upload_approval_mode, disable_auto_review, full_cdp_access_enabled, webmcp_enabled), rule tables | browser/config.toml, /wham/browser/settings |
| Comment | type comment, text, localBrowserContext (page, frame path, role, name, selector, rect, nearby text, selected text, virtual target, document context), additionalSelections, designChange, screenshot, attached images | src-CP7 `La()` |
| WebMCP tool | name, call_name, registration_id, title, description, input_schema, annotations {readOnlyHint, untrustedContentHint, consequentialHint}, origin, pageUrl | main `eK` |
| Security decision | check, outcome, permissionSource, reason, origin, durationMs, backend | browser-service `Yt` |

## 4. Notable design decisions

1. Code over tools: let the model batch many actions in one call; push safety into the runtime and host instead of into tool granularity (F075).
2. One command protocol, many backends, with explicit per-backend support flags rather than lowest-common-denominator behavior (F076, F077).
3. Fail closed everywhere: unavailable policy checks deny, retryable flags distinguish temporary from permanent blocks, every denial tells the model not to work around it (F091).
4. Authorization answers are produced by the host and signed; model JS can ask but not forge (F109).
5. Secrets are never model-visible: broker fill plus observation gate plus protected evaluation (F101, F102, F114).
6. Page-originated content is data: annotation metadata and page prompts are shown for user review; WebMCP tool text cannot authorize; site instructions only via the trusted proxy (F064, F097, F014).
7. Error messages are part of the API: raw CDP refusals name the high-level replacement (F096); locator failures include match counts (F083); required docs errors name the exact call (F001).
8. Lifecycle is turn-scoped: temporary agent tabs die with the turn unless marked; clipboard and debugger leases are restored at turn end (F088).
9. Background first: agent browsing is hidden by default, with paint leases for capture and no throttling for local pages (F086, F052, F068).
10. Real browser, real profile: rely on Chromium features and the user session; no stealth or fingerprint evasion was found (F011).

## 5. What is best-in-class (versus Atlas-style, Aside-style and Firecrawl/Apify-style peers)

- Safety taxonomy and consent matrix (F091, F093): more complete than anything in Firecrawl or Apify, and more granular than typical browser agents.
- Credential isolation (F101, F102, F103, F114): the model never sees secrets and cannot observe the page after native fill.
- WebMCP integration (F097) and Site Annotation API (F063): a two-sided contract between sites and agents that avoids UI clicking.
- Annotation and page-context UX (F059 to F062): structured targets, batch review, design diffs.
- Typed, capability-negotiated protocol with doc gating (F076, F077, F001).
- Operational hygiene: safe/sensitive logging (F024), decision audit (F025), debug ring buffer (F072).

Where Codex is not ahead: bulk acquisition (no batch fetch for its own browser (F002), no crawl, queue, schedule, change monitoring (F018)); stealth, proxy rotation and CAPTCHA handling (F011, F014 is cloud-only); output formats beyond screenshot, AX and DOM (no markdown or structured LLM extraction in the browser stack). Firecrawl, Apify and Quarry lead there.

## 6. Weaknesses, limitations and risks

1. Windows pipe peer authorization is absent (`cp()` returns authorized unless darwin) (F078). Pipe DACL not inspected. Risk: local process drives the browser.
2. Site-status check is fail-open on request errors and sends origin+path with conversation and turn ids to OpenAI (F094).
3. One shared persistent profile for all tabs (F030); consent prompts are the only separation between agent and user sessions.
4. `x-openai-site-instruction` is trusted when present on responses in the operator-proxy configuration; a malicious origin could set it if the proxy does not strip upstream copies (F014).
5. zxing-wasm default `locateFile` points to `fastly.jsdelivr.net`; a shipped WASM also exists, override not located (F008).
6. Large surface to maintain: Chromium fork features, owl-specific APIs, three runtimes (client, service, host) and a policy file that is synced; conflicts surface as a generic message (F056).
7. Several features are gated off by default (AX in IAB, history, WebMCP, PiP, tabs strip unification) so parity targets may not be fully shipped (F020, F080, F069).
8. The policy layer is prompt plus code; the confirmation taxonomy (F105) is advisory for items outside the check list.
9. No batch or crawl; the agent browser is serial and model-driven (F002, F018).
10. Cloud Browser details (engine, hosting, proxy provenance) are invisible from the client (F067, F014).

## 7. Mapping to Verevon and Quarry v2

Baselines used: `v-browser-app.baseline.md` and `v-quarry.baseline.md` (Audit 2 scout outputs). Facts about Verevon below come from those baselines (SRC/INF there), not re-verified.

### 7.1 What Verevon already has that Codex built on top of
- Quarry `BrowserSnapshot` with `@eN` refs, AX projection, frame refs, semantic locators, `respond_dialog`, upload/download refs gated by one-shot grants, session limits, ZDR-aware profile scopes [baseline]. Codex's equivalents: F080, F083, F084, F098, F099, F108.
- Human takeover toggle, devtools drawer, timeline and evidence drawer, approvals list, profile popover [baseline]. Codex equivalents: F090, F067, F106, F025.

### 7.2 Gaps ranked (copy)

| # | Gap in Verevon/Quarry | Codex feature | Recommendation | Effort |
|---|---|---|---|---|
| 1 | Model-facing `browser.observe/act` exist but are unreachable because nothing issues BrowserBroker grants; UI uses the older selector/coordinate vocabulary | F080, F082, F083, F076 | Fix grant issuance first (Model Plane), then expose Quarry snapshot refs and AX in the UI and agent; add AX diff output and element-index semantics | M |
| 2 | No typed denial reasons; errors are generic | F091, F096 | Add a reason enum (approval, policy, enterprise, site_status, navigation_policy, user_declined, guardian) with retryable flag and no-workaround text to `browser.act` errors; remediation hints for blocked actions | S |
| 3 | BrowserBroker allowlist is at most 32 hosts without wildcards; no per-capability consent | F093, F071, F056 | Per-origin matrix (browse, download, upload, debug, site tools) with `*.` and `**.` patterns, org-managed overrides and a revisioned account/org document | M |
| 4 | No credential vault; login triggers HITL; cookie snapshots unencrypted | F101, F102, F103, F114 | Credential broker: model supplies selectors, secure form fills, status only; observation gate until new document; encrypt profile snapshots; protected-field policy in any evaluate | L |
| 5 | No agent cursor or element highlight; only rationale bubbles | F085, F086 | Cursor overlay with move sequence and arrival ack in the live view; element highlight for the target of each action | S |
| 6 | Comment/annotate on page is a stub | F059, F060, F061, F049 | Real annotate mode: element/text/region anchors using Quarry refs, batch review, send as structured context; @tab mention with title+URL staleness check | M |
| 7 | Takeover is UI-enforced, advisory server-side | F090, F088 | Server-side control lock, typed `declined user_took_over` result, turn-scoped tab marks (deliverable/handoff) | S |
| 8 | evaluate denied entirely | F114 | Read-only evaluate in an isolated world with credential-field protection and result caps | M |
| 9 | No page-published tools | F097, F063 | WebMCP discovery over CDP with descriptor limits and per-origin allow lists; consider a first-party annotation/WebMCP contract for Verevon apps | M |
| 10 | SSRF guard only syntactic at gateway | F095 | Enforce scheme/policy on every request, redirect and subframe in the browser itself, emit navigationBlocked to the model | S |
| 11 | No consent-gated downloads UX or grants with TTL | F098, F042 | URL-bound short grants, blob origin binding, download manager list | S |
| 12 | Sessions 12 steps, 120 s, one domain | F088, F017, F034 | Turn/run-scoped tab lifecycle, tab budget with suspension, snapshot-based resume | M |
| 13 | Audit trail of actions | F025, F106 | Emit decision events with check, outcome, source, reason; agent change audit with before-state | S |
| 14 | Omnibox and shell absent (history, downloads, find, zoom) | F036, F037, F039, F042, F043 | Only if a standalone browser surface is a product goal; otherwise keep the panel minimal and focus on agent features | L |
| 15 | Local-to-cloud trust | F109, F120 | Signed authorization answers from the control side instead of bearer tokens visible to model code | M |

### 7.3 Copy now, copy later, avoid

Copy now (cheap, high value): F091 typed reasons, F096 raw CDP remediation hints, F083 locator diagnostics, F088 deliverable/handoff marks, F085 cursor, F025 audit events, F001 docs gating, F024 safe/sensitive logging.

Copy later (needs design): F093 consent matrix, F101/F102 credential broker, F097 WebMCP, F114 read-only evaluate, F060/F061 annotation, F077 capability matrix.

Avoid or adapt: Electron/owl-specific tricks (paint lease, webContents.clone, webview adoption) that do not apply to server-hosted Chromium (F028, F029); a shared profile for agent and user (F030) when Quarry already has scoped profiles; fail-open site checks and sending URL paths to a third party (F094) under ZDR; local pipe without peer auth (F078); CDN-default WASM loaders (F008); relying on prompt-level confirmation taxonomies where a server check can enforce (F105).

### 7.4 Quarry v2 engine implications
- Quarry should expose a per-backend support matrix and capability docs (F077) so the gateway stops hiding actions silently.
- Adopt tiered action selection (AX index, DOM node id, locator, coordinates) with failure diagnostics (F082, F083).
- Keep bulk acquisition (batch, crawl, map, schedule) in Quarry and Ingestions; Codex shows the browser is the wrong tool for that (F002, F018).
- Consider an engine-tier model like Codex's four backends: hosted headless, hosted headful live view, user-session extension, MCP/app widgets (F009, F010).
- Port-forward or tunnel for sandbox localhost previews (F051) is directly relevant to Model Plane sandboxes.

## 8. Open questions

1. What is the Windows named-pipe DACL, and can a different local user or process connect (F078)?
2. Does the cloud proxy strip upstream `x-openai-site-instruction` headers (F014)? Where does the site guidance database live?
3. Does the AX text engine ship enabled for the IAB (TinySky gate) or only for extension and cloud backends (F080)?
4. Which backend, if any, enables `Tabs.content` (F002)?
5. How are Agent-tagged history entries stored and exposed (F043)? Is the tag per navigation initiator only?
6. What does the `/flora/browser/form-fill` endpoint do and who calls it (F046)?
7. Is the Local Work executor path (cloud agent driving the local browser) generally available and what consent does the user give (F109)?
8. Is zxing loaded from the CDN at runtime or from the shipped WASM (F008)?
9. What is the full set of annotation permission defaults and how are page-suggested prompts sanitized (F063, F064)?
10. How does Chromium `WebMCP.*` CDP domain behave across versions (F097)?

## 9. Evidence index (starting points)

- Main process: `main.pretty.js` L36712 (attach), L36858 (partition), L36864 (hardening), L40954 (IAB backend), L41308 (CDP execution), L41783 (CDP method guard), L5737 (navigation restriction), L43059 (WebMCP), L44129 (tab budget), L44630 (host manager), L17975 (policy schema), L18238 (cloud sync), L42008 (session registry), L19133 (profile import), L44409 (permission prompts), L42676 (context menu), L45529 (remote localhost forwarding).
- Preload: `preload.pretty.js` L3716 (channels), L3748 to L3984 (Site Annotation API), L15062 and L15850 (comment runtime), L16001 (WebMCP listener).
- Plugin: `browser-service.pretty.js` L1130 (site status), L11376 (audit), L11480 (security modes), L11546 (origin consent), L11684 (WebMCP approval), L14656 (browserAuth), L18733 (raw CDP policy), L20116 (read-only evaluate), L23498 (site instructions), L24418 (virtual clipboard), L25248 (doc gating); `browser-client.pretty.js` L7 (denial taxonomy), L131 to L134 (command catalogue).
- Docs: `PLUG/browser/docs/*.md`, `api.json`, `documents.json`; `PLUG/computer-use/docs/*.md`.
- Renderer: `webview/assets/app-shared-737655e1fb23.js` (i18n catalog), `browser-use-policy-site-permissions-124f08931952.js`, `cloud-browser-*`, `hidden-browser-use-webview-host-*.js`, `annotation-*`.
