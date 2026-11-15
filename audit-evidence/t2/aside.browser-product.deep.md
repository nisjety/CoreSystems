# Aside browser 1.0.928.2: user-facing browser product, deep dive

Facet: browser-product. Source: the installed Aside browser (Windows x64, publisher At Your Side Inc.).
Taxonomy focus: category 8 (in-app browser shell) and category 9 (AI-browser control), with the few records from other categories that the browser product depends on.
Companion file: `aside.browser-product.features.json` (101 records, ids `aside-browser-product-001` to `-101`).

Provenance tags used below: **[SRC]** read directly in installed program files, **[REA]** produced by an REA tool, **[DOC]** documentation or shipped skill text, **[INF]** my inference (basis stated). Confidence is stated where it is not high.

---

## 0. Read this first

### 0.1 What was analysed, and how

Read-only analysis of program files. Nothing was launched, installed or run; no per-user data directory was opened.

| Alias | Meaning |
|---|---|
| APP | `C:/Program Files/Aside/Application/1.0.928.2` |
| AM | `APP/AsideAgentManager`: MV3 extension "Aside Browsing Agent" (side panel, new tab, tab search, mini popup, lasso, remix, bridge). `background.js` is 11,836 lines and was read in full where it mattered; line numbers in the JSON refer to it. |
| PM | `APP/AsidePasswordManager`: MV3 extension "Aside Vault" |
| DMN | `APP/AsideDaemon/win-x64`: `aside-daemon.exe`, a Node single-executable app (about 14 MB of embedded JS), plus `native/`, `computer-use/`, `sandbox/`, `static/skills/builtin` (32 skills) |
| BLOB | scratch copy of the daemon's NODE_SEA_BLOB resource (`scratchpad/tmp/aside/daemon_sea_blob.bin`). Evidence cites `BLOB@<byte offset>`; offsets are reproducible with `indexOf` on that file. |
| DLL | `APP/Aside.dll` (306.7 MB Chromium 153 core with the Aside C++ code); strings were extracted with byte scans |
| PAK | `APP/Locales/en-US.pak`, decoded to a TSV in scratch (10,408 entries); cited by string id |

### 0.2 REA usage (honest account)

REA tools really used in this pass:

- `analyze_javascript_application` (directory mode, summary, record-and-continue) on a copy of the AM core files. Result: partial graph (the worker heap limit was hit on `background.js` and `content.js`), 242 findings, 0 modules. Retained evidence id `ev_d6c88618...`.
- `analyze_javascript_application` on a copy of six PM page scripts. Coverage complete, 3 findings. Retained evidence id `ev_93afce80...`.
- `trace_application_feature` four times (seeds `extension-bridge`, `chrome.debugger.getTargets`, `ASIDE_FIDO2_CREATE_REQUEST`, `credentials`). All returned no match even though the literals exist in the files. REA's string and API seeds do not index these minified MV3 bundles, so absence of a match is not evidence of absence.
- `inspect_pe_resources` on `Aside.dll` (105 resources, 25 icon groups; evidence `ev_36286a93...`). Low value (icons, manifest, version, event template).
- `search_strings` and `open_binary` were not used: the REA active target is shared by all agents and a 306 MB module is slow to load. Strings came from byte scans (`grep -a`) and a small pak decoder I wrote in scratch, so those claims are tagged SRC, not REA.

The honest summary: REA was a weak instrument for this target; almost every finding comes from reading beautified extension code, the daemon payload and locale strings directly.

Housekeeping note: early in the pass I wrote nine small helper scripts and one tiny empty folder into the user Temp directory by mistake. I moved the scripts into the scratchpad (`scratchpad/tmp/asidebp/tools`) and removed the empty folder. Nothing else outside the scratchpad was written.

### 0.3 Corrections to the scout inventory (`aside.inventory.md`)

| Inventory said | What the code shows | Evidence |
|---|---|---|
| `chrome.runtime.connectNative` carries the extension to daemon handshake (register, connectionParams, pong) | `connectNative` is used only for the Apple Passwords helper (`com.apple.passwordmanager`). The extension to daemon link is a WebSocket `/extension-bridge/extension` with `register`, `ping`/`pong`, `runCommand`, `cancelCommand`. `connectionParams` belongs to the tRPC client. | AM `background.js:8406,8619,9840-10037` |
| No dedicated prompt-injection defence | An untrusted-tool-output envelope with tag neutralisation exists, conditional on model tier, plus explicit instruction-boundary text in the system prompt | BLOB@6339944-6343164, @6360511 |
| Desktop computer-use actions (`mouse.*`, `keyboard.*`) in the helper | Those names are Context Awareness event kinds (passive capture from `aside_dom`, `mac_ax`, `win_uia`). On Windows the helper serves capture; `runComputerUseCommand` is macOS-only (iMessage send, permission probes). `computerUse` strings are from the vendored Google GenAI SDK. | BLOB@6828385, @10385760, @10402147 |
| `mcp-tunnels` literal suggests MCP tunnels | The literal comes from the vendored Anthropic SDK beta headers, not from Aside | BLOB@3467847 |
| CLI: none shipped | True for this install, but the product ships a Developers page that installs an `aside` CLI (`exec`, `repl`, `mcp`, `host`, `login`, `skills install`) and the daemon has the `cli.repl*` procedures | AM `assets/developers-DJXFtMQS.js`, BLOB@11370960 |
| Daemon wire protocol unknown | Mostly resolved: tRPC over HTTP and WebSocket on 127.0.0.1:21420, WebSocket bridge, challenge-response auth, authenticated CDP on localhost:45103 | sections 1 and 4 |
| Playwright or Puppeteer absent | Confirmed absent as runtimes; the agent API is an in-house Playwright-compatible layer ("Asidewright") on raw CDP | BLOB@3161686 |

---

## 1. System shape

```
 +--------------------------- Aside browser (Chromium 153 fork, Windows) ----------------------------+
 |  Aside.dll: native views (vertical strip, chats, task popover, tab switcher, NTP prerender, ...)   |
 |  Private extension APIs: asideAccount, asideOmnibox, asideAdblock, asideNotification,             |
 |                          asideBrowserImport, asideBrowserPreferences                              |
 |  Authenticated DevTools endpoint  localhost:45103  (/json/challenge -> /json/auth/session)        |
 |                                                                                                   |
 |  Extension "Aside Browsing Agent" (AM)         Extension "Aside Vault" (PM)                       |
 |   side panel, new tab, tab search, mini popup,  autofill, passkeys, TOTP, generated-password refs |
 |   lasso, remix, capture, translate, bridge   <--externally_connectable (mutual allowlist)-->      |
 +-------------|-------------------------------------------------------------------|------------------+
               | WebSocket /extension-bridge/extension (Aside.* commands)          |
               | HTTP+WS tRPC, challenge-signed token       CDP over WS (token)    |
               v                                                                   v
 +------------------- aside-daemon.exe  (Node SEA, 127.0.0.1:21420) -------------------------------+
 |  agent loop, REPL (QuickJS WASM), Asidewright (Page/Locator over CDP), snapshot+diff, tools,     |
 |  permissions, routines, memory, context-awareness ledger, inbox, MCP client, channels, billing   |
 |  stores: settings.json, state.db (SQLite), remote-control.json, context-awareness/ledger.db       |
 +--------|------------------------|------------------------------|-----------------------------+
          | stdio                  | stdio                        | HTTPS / WSS
          v                        v                              v
  AsideComputerUse.exe     AsideWindowsSandboxHelper.exe     api.aside.com (search, tab-tidy plan,
  (context capture)        (AppContainer)                     remote-control relay, billing, auth)
```

Key facts [SRC]:

- The agent runs in a **local daemon**, not in the browser and not in the cloud. The browser exposes two control channels to it (CDP for the DOM, an extension bridge for shell-level commands). The cloud is used for model calls, web search, tab-tidy naming, billing and the remote-control relay.
- Everything is routed by a key of **(accountId, profileId, browserMode)**. Commands sent to the wrong profile or to default versus incognito mode are refused by the extension (AM `background.js:9635,9976-9990`).
- Ports: daemon 21420 (21421 in dev), authenticated CDP 45103, a dev web app on 5175 is in the CORS allowlist.
- Data stores [SRC]: per-account `settings.json` validated by one schema, SQLite `state.db` (sessions, runs, turns, `session_tabs`), `remote-control.json`, `context-awareness/ledger.db`, memory files (`AGENTS.md`, `SOUL.md`, `memory/USER.md`, `memory/MEMORY.md`), vault DB and audit logs.

---

## 2. The browser shell (category 8)

### 2.1 Tabs, windows, sidebar

- **Vertical tab strip with Chats and Tasks sections.** Chromium 153 already has vertical tabs; Aside adds native sections for Chats (New Chat, pinned and recent chats with throbber, status dot and badge), a Tasks area, bookmarks represented as tab rows, a profile pager and a Tidy action (`vertical_tab_strip_region_view.cc`). The strip reads `GET /session/for-chrome/chats` from the daemon and shows "Awaiting approval" and "Awaiting answer" states. Chat context-menu verbs: pin, unpin, rename, mark read, mark all read, archive all, open folder, copy session id. [SRC] (records 002, 003, 016, 090)
- **Agent tabs are first-class.** A dedicated "Agent Tabs" group holds tabs the agent opens; the strip shows a banner "Aside is controlling the tab" with a Take over button; there is a second group for tabs used by external agents; "Clear tabs" spares AI tabs; agent tabs auto-close after an idle time (default 15 min, range 5 to 1440); duplicates are merged. Aside.dll has `AsideAiTabsTakeOver` and `AsideAiTabsViewport` inside `multi_contents_view.cc`, so the agent tab is probably shown beside the user's tab in the multi-contents (split) layout [INF, medium]. (record 062)
- **Tab groups** carry a colour and an Aside icon (48 named icons). `chrome.tabGroups.update(id, {icon})` is an Aside patch. (record 006)
- **AI Tab Tidy** (record 007) is the best-specified AI feature in the shell. The native button sends `aside.verticalTabsTidy`. The extension: collects non-pinned, ungrouped http(s) tabs of the focused window; dedupes by a normalised URL (tracking parameters stripped, hash kept only if it looks like a route); keeps the best duplicate (active, highlighted, audible, most recently accessed); clusters locally (same host, same registrable domain plus first path segment, or at least two shared tokens; batches of at most 40); sends each batch (title, URL, host, path, tokens) to `https://api.aside.com/tabs-tidy/plan`; applies the returned `{title, icon, tabIds}` groups; moves the Agent Tabs group to the front; reports phase timings. If the planner fails, tabs are simply left ungrouped. Privacy cost: tab titles and URLs go to Aside's cloud. [SRC]
- **Split view** (side by side or stacked, pinnable) and **tab switcher** are upstream or native Views. **Tab search** is an extension page in a native bubble that searches tabs, recently closed tabs, chats, history, bookmarks and extensions in one box. (records 005, 008)
- **Live tab previews**: a player page renders a MediaStream from `chrome.tabCapture.getMediaStreamId`; one subscriber owns a target at a time and others get explicit fallback reasons (`not-visible`, `owned`, `no-tab`, `capture-failed`). (record 009)
- **Profiles**: each browser profile binds to an Aside account (`/accounts/profile-binding`); the sidebar can page between profiles with snapshot previews (profile pager). (record 032)

### 2.2 New tab page and omnibox

- The new tab page is an extension page. Its first paint is static HTML plus a permanent vanilla Tiptap editor so typing works before React mounts; the theme and sidebar column are resolved before first paint. Native code prerenders it (`AsideNtpPrerender`) and animates the swap. (records 010, 091)
- One box, two modes, switched with Tab: **Search or type a URL** and **Ask Aside a task, @ for context**. `@` scopes (bookmarks, history, tabs, assistant) show as chips. Search mode uses the browser's real address-bar engine through the private `chrome.asideOmnibox` API (createSession, queryAutocomplete, openPopupSelection, addTabContext, addFileContext, getRecentTabs, setActiveToolMode, setActiveModelMode and about 25 events). Ask mode mounts the same composer as the side panel and creates an agent session. Dropping an image offers Lens search. (record 011)
- A "Connect Aside to Codex / Claude Code" nudge appears when the session provider is a coding agent. (record 045 notes)

### 2.3 Side panel, mini popup, native task popover

- **Side panel**: per tab, opened with Alt+Shift+B; the panel URL carries the account and session id. On tab activation the extension resolves the tab target to a session (`sessions.tabs.resolveByTarget`) and updates `activeTabTargetId`; a setting chooses **new chat per tab** (default) or **keep current chat**. The agent is told it is a sidecar to the current tab and may edit that page with `page.evaluate`. (records 012, 013)
- **Mini popup**: a small floating composer from a shortcut, with attachments, incognito, project, browser profile, permission mode, Final confirm and memory toggles, and a model plus reasoning selector (seen in the marketing image). (record 014)
- **Native task popover**: the shell polls the daemon, shows task rows and attention badges, and posts `resolve-popover-action {popoverId, actionId, sessionId}`; popover payloads come from the daemon (draft approvals and permission requests). (record 015)
- **Presence**: the agent UI reports `{focused, visible, engaged, sessionId}` with a heartbeat so the daemon can suppress notifications for the session the user is watching (TTL 30 s inactive, 60 s active). (record 097)

### 2.4 Page context and page actions

- **Attachments**: tabs (with targetId), files, folders, text selections and region selections (summary, preview image, element list with selector, tag, role, text, alt, src, href). Chat passages can be selected and commented as "response annotations". Selection state per tab lives in the service worker and is cleared on navigation or tab close. (record 017)
- **Lasso** (records 018, 019): a modifier double-tap (Alt by default) starts selection; text selection and region selection produce a small tool strip; defaults are Summarize, Translate, Copy code, Search image; users add shortcuts with `prompt`, `open-url` or `copy` actions and placeholders `{text}`, `{image}`, `{code}`; results stream from `POST /lasso/transform`; text can be applied back into inputs, and Google Docs has a MAIN-world selection bridge. `react-grab` supplies component source for the selected element. The marketing image shows a freehand circle drawn around an element.
- **Remix** (record 020): per-host user scripts (https, exact host, up to 20 scripts of 50 KB) registered through `chrome.userScripts`; the script API includes `aside.prompt()`, which starts an agent session with the current tab attached; Gmail and Slack ship built-in remix scripts. This is the most original shell idea in the product.
- **Capture** (record 021): viewport, area or full page; viewport and full page go through CDP in the daemon, area uses `captureVisibleTab` plus a canvas crop.
- **Page translation** (record 022): fully on-device via the Chrome Translator API; text nodes and attributes are grouped, translated in batches and replaced in place.
- Upstream selection actions (Preview, Explain, Ask about this page, Lens) are also present. (record 093)

### 2.5 Passwords, passkeys, autofill (records 035 to 039)

- **Aside Vault** is a separate extension: libsodium crypto, multiple vaults with sharing and key fingerprints, 20+ item categories, TOTP, generated passwords, breach check (k-anonymity range API), CSV/JSON import and export, recovery key file, biometric unlock (Windows Hello via `aside-native.node`), and Windows credential prompts before reveal, copy, edit or export.
- **Autofill** has inline suggestions, cross-frame card fill, OAuth login items, TOTP wait states and an HTTP block. **Passkeys** are handled by a page-world WebAuthn bridge with conditional UI and fallback to the native authenticator.
- **External managers** (1Password, Bitwarden, Dashlane, LastPass, Proton Pass, Apple Passwords) can be connected; the agent prefers a connected provider and falls back to Aside Vault. Apple Passwords is reached with an SRP-style key exchange over native messaging.

### 2.6 Privacy, blocking, activity capture

- **Ad and tracker blocker** (record 027): private `chrome.asideAdblock` API with subscriptions (uBlock and EasyList lists), custom rules up to 4 MiB, cosmetic and procedural filters, diagnostics and a request tester.
- **Context Awareness** (records 040 to 042): opt-in, Max plan. Captures page snapshots, window changes, clicks, shortcuts, typed-text length, selections and screen OCR from the DOM, Windows UIA and macOS AX; retention 1 to 30 days (default 14); never-observe rules per app or website domain; pause; summaries by a background model; the agent reads it through `history_search`, `context_explorer` and `memory_search`. **Capture-time redaction** replaces password and phone inputs, sensitive autocomplete tokens, secret-labelled fields and secret-shaped values (JWT, AWS key, API key, Slack token, private key block, OTP digits, Luhn-valid card numbers) with `[redacted:capture]`. The extension also sends lightweight tab signals (activation, 2 s dwell, audible), never for incognito.
- **Incognito for agents** (record 043): drops memory and history tools, keeps the session out of lists, expires it after 30 days.

### 2.7 Account, sync, import, notifications, updates

- Aside account with daemon-held sync encryption key (`/auth/sync-passphrase`), cookie and website-storage sync services; daemon flag `experimental.sync` is false by default, so enablement is unverified. (record 033)
- Importer WebUI and `chrome.asideBrowserImport` for Arc, Atlas, Chrome, Comet, Dia, Edge, Firefox and Safari. (record 026)
- Notifications: Windows toasts with click paths and custom sounds; **website web-push notifications are captured into a daemon inbox** and can trigger routines. (records 034, 069)
- Updater is a component updater gated on daemon health; relaunch prompts; default-browser and taskbar nudges. (records 030, 075)

---

## 3. AI-browser control (category 9)

### 3.1 Two control channels, one transport abstraction

1. **CDP** to the user's browser at `localhost:45103`. The endpoint is **authenticated**: the daemon requests `/json/challenge`, signs it with a per-installation key and exchanges it at `/json/auth/session` for an `AsideSessionToken` that is required for `/json/version` and the WebSocket. This closes the usual hole of an open remote-debugging port. (record 050) [SRC]
2. **Extension bridge** for things CDP cannot do: tab lifecycle in the user's window, Agent Tabs grouping, downloads, notifications, cookie policy, vault commands, history search. Commands are `Aside.*` methods with typed schemas, per-command abort, ping/pong and a 30 s keepalive alarm. (records 047, 048)
3. The page layer calls non-standard methods such as `Aside.waitForDownload` through the same `send()` as CDP, and the transport picks the channel (`{extensionBridge: route}`). (record 087)

### 3.2 Tool protocol: one code-first REPL

- The primary tool is a **persistent Playwright-style REPL** (ES2023, QuickJS WASM sandbox, 120 s timeout, 16 KB output cap). Globals: `page`, `tabs`, `listBrowserTabs`, `attachBrowserTab`, `attachActiveBrowserTab`, `openTab`, `closeTab`, `snapshot`, `annotatedScreenshot`, `display`, `fetch` (with the user's cookies), `fs`, `aside` and per-skill globals (`passwordManager`, `captcha`, `cua`, `chrome`, `channel`, `imagegen`, `linkedin`, `googleSearch`, ...). Structured tools exist beside it (bash, read_file, write_file, edit_file, grep, glob, terminal, skill, websearch, webfetch, memory and history tools, send_message, ask_user_question, request_action_confirmation, write_todos, subagent, routine_update). (records 052, 088)
- **Asidewright** is an in-house implementation of Page, Locator, FrameLocator, ElementHandle, mouse, keyboard, file chooser, download and screencast video over raw CDP sessions with frame managers and isolated worlds. There is no Playwright or Puppeteer runtime. Locators accept snapshot refs directly. (record 053)
- **Snapshots** are the main perception: an accessibility tree with refs like `e12` and frame-prefixed refs like `f1e5`, iframe origin labels, shadow DOM traversal, 300-character accessible names, scoped snapshots (`selector`) and interactive-only mode. After an action the REPL shows a **Myers line diff** of consecutive snapshots ("No changes detected" when equal), so the model rarely re-reads the page. Stale refs raise a typed error telling the model to re-snapshot. Password field values show as `[redacted]` even to the agent. (record 054)
- Fallbacks: `annotatedScreenshot` (PNG with ref labels) and `cua` coordinate actions, with written rules for when not to use them. (record 055)
- Reliability details: agent tabs keep `Page.setWebLifecycleState=active` and focus emulation on, use a fixed 1440x900 viewport and can auto-accept dialogs (record 085); `webfetch` falls back to a real browser tab on Cloudflare challenges and converts HTML with a readability library (record 083); site playbooks (16) inject canonical URLs and snapshot selectors by URL or keyword (record 066); several services are reached through site APIs with the user's cookies (record 067).

### 3.3 Safety and approvals

| Layer | Mechanism | Record |
|---|---|---|
| Session mode | `read-only`, `guard` (default), `full-access` | 056 |
| Rules | allow, approved, deny, ask, default; matchers for tool (glob plus argument eq/regex), browser (`read`, `modify`, `download` with URL glob) and network (domain glob); file roots with outside read and write set to deny or ask; sandbox on by default; **deny beats ask beats allow** | 056 |
| Enforcement | Page methods call `ensureAccess(action, url)`: modify on goto, back, forward, reload, openTab, popup; read on content, screenshot, snapshot; download on downloads; `webfetch` checks every redirect | 057, 083 |
| Final confirm | optional per session; the agent must call `request_action_confirmation` before external, destructive, paid or irreversible actions, with a destination-shaped review card (Gmail, iMessage, LinkedIn, Slack, calendar, X drafts) or a screenshot; must be the only tool call in the turn; re-ask if the page, recipient or amount changes | 058 |
| Payments | `paymentUse`: default off; ask, auto-approve under USD (default 50) or never; per-card usage notes; default card; daily or monthly cap with ask or block | 059 |
| Prompt injection | untrusted-output envelope plus tag neutralisation for models outside a high-capability list; system prompt: pages and tool output are data, `<system_message>` only counts in the user turn | 060 |
| Credentials | opaque `GeneratedPasswordRef`; `autofillItem` fills logins, cards and identities without exposing values; vault accepts messages only from the agent manager extension id; refs held in memory | 061 |
| Browser protection | raw tab, window and tab-group mutations unavailable to the agent; `skipIfActive` stops the agent closing the tab the user is on; separate Agent Tabs group; takeover banner | 048, 049, 062 |
| Storage hygiene | capture-time redaction; incognito sessions; never-observe rules | 041, 043 |
| Execution | QuickJS sandbox, AppContainer helper for commands, explicit read and write roots, Remote MCP over HTTPS only with no redirects | 076, 080 |

### 3.4 Multi-tab, multi-session and sub-agents

- A session has a `browserBinding {profileId, browserMode, windowId, anchorTargetId}` and owns several tabs (`session_tabs`); closing a tab removes the session tab. (record 063)
- Sub-agents with profiles (`default`, `custom`, `context_explorer`, `code_explorer`) run with fresh context and may be read-only; a side chat can answer questions about a running task but must not drive its tabs. (record 065)
- The tool list is derived from session kind (read-only subagent, headless routine, incognito, side chat). (record 088)

### 3.5 Background, event-driven and remote use

- **Routines** run on a schedule (once or RRULE) or on events (web push from a site origin, webhook, email, agent message). `page.grantNotificationPermission()` grants an "Allow for AI" permission silently, the agent opts the user in on the site, and the routine waits for the push. Aside also mines browsing history to suggest routines. (records 069, 070, 101)
- **CLI and MCP** (record 071): `aside exec` delegates a task, `aside repl` gives Playwright-like control, `aside mcp` serves tools to Codex, Claude Code, Cursor or OpenCode, and `aside skills install` writes the browser skill into those agents. The pitch is that credentials and cookies stay in the browser.
- **Remote Control** (record 072): Pro and Max. The daemon enrols as a host, keeps a WebSocket to the Aside API, and serves only an allowlisted set of tRPC procedures and WebSocket paths (sessions, projects, models, `cli.repl*`; agent chat, session sockets, event bus `session:*`, session files). Envelopes over 100 KB are segmented with a 32 MB reassembly cap.
- **Durability** (record 089): after a daemon restart, tools that are safe to repeat are re-run (reads, search, fetch, questions); other interrupted calls return "state unknown, check before retry".

### 3.6 What is not there

- No agent cursor or page overlay in the JS layers (no named component; the marketing images show a labelled cursor, so the native layer may draw one [INF, low]). [SRC]
- No domain allowlist object as such; the same effect comes from `browser` and `network` permission rules with URL globs.
- No robots.txt handling, no proxy rotation, no stealth or fingerprint randomisation, no crawl, map or batch API. Aside is a user agent acting for one person, not a crawler.
- No agent-side desktop computer use on Windows in this build (record 082).

---

## 4. Protocol and data reference

### 4.1 Extension bridge commands (daemon to extension) [SRC] (AM `background.js:9634-9830`)

`Aside.listTabs`, `ownsTarget`, `openProfileUrl`, `createAgentTab`, `resolveBindingWindow`, `searchHistory`, `getThirdPartyCookies`, `setThirdPartyCookies`, `ensureTabInAiTabsGroup`, `controlTab` (open | close with `skipIfActive`), `waitForDownload`, `showNotification`, `playNotificationSound`, `grantNotificationPermission`, `revokeNotificationPermission`, `clearAiTabsGroup`, `callExtensionMV3Api` (32-method allowlist), `createPasswordManagerGeneratedPasswordRef`, `fillPasswordManagerGeneratedPasswordRef`, `resolvePasswordManagerGeneratedPasswordRefs`, `clearPasswordManagerGeneratedPasswordRefs`, `performPasswordManagerAutofill`, and `applePasswords.capabilities | requestAuth | verifyAuth | listLogins | getPasswords | saveLogin | getOtps`.
Envelope: `{method:"register", connectionId, route:{accountId, profileId, browserMode}}`, `{id, command, route}` in, `{id, result|error}` out.

### 4.2 Daemon HTTP and WebSocket surface (127.0.0.1:21420) [SRC]

- Auth: `GET /auth/daemon/challenge?clientKind=`, `POST /auth/daemon/session` (HS256 JWT, audience `aside-daemon-localhost`), `/auth/access-token`, `/auth/sync-passphrase`; every request must carry a loopback Host header.
- tRPC at `/trpc` (routers: agentRuntime, accounts, projects, analytics, billing, capture, channels, cli, contextAwareness, developers, inbox, lasso, sessions, settings, suggestedItems, sync, uiAttention, memory, mcp, models, onboarding, fullDiskAccess, passwordManager, externalPasswordManagers, referrals, remoteControl, routines, meta).
- Streaming and REST: `/agents/chat/{id}/{stream|steer|continue|abort}`, `/ws/sessions/{id}`, `/ws/event-bus?pattern=session:*`, `/lasso/transform`, `/sessions/{id}/files`, `/extension-bridge/extension` (WS) and `/extension-bridge/command` (HTTP fallback), `/health`, `/shutdown`.
- Browser-chrome paths without bearer (loopback only): `/session/for-chrome/{recents|chats|search|projects}`, `/{id}/{move-to-project|state|rename|archive}`, `/resolve-popover-action`.
- Browser side: `localhost:45103/json/challenge`, `/json/auth/session`, `/json/version`, CDP WebSocket.

### 4.3 Runtime message names (extension internal) [SRC]

`capture:open|capture`, `lasso:transform:start|chunk|complete|error|abort`, `lasso:get-settings|get-account-id`, `remix:prompt|get-enabled|sync`, `translate:page-start|page-status|page-cancel|page-dismiss|selection|toast-change-languages|toast-close`, `page-selection-context:get|update|subscribe|event`, `extension-bridge:ensure|keepalive`, `daemon-auth:sign-challenge`, `extension-notification:show|play-sound|offscreen-ready|clicked`, `current-zoom-indicator:show`, browser-originated `aside.verticalTabsTidy`, `aside.inboxNotification`, `aside.notificationPermissionChanged`.

### 4.4 Settings keys that shape behaviour [SRC] (BLOB@1679567-1690291)

`defaultPermissionMode` (guard), `followUpBehavior` (queue|steer), `sidepanelSessionOnTabChange` (new-session|keep-session), `sidebarRecentChatLimit` (3|5|10), `agentTabs.closeAfterIdleMinutes` (15), `contextAwareness {enabled, pausedUntil, rules[], retentionDays, captureTypedText, screenOcr, summaryModel}`, `paymentUse {...}`, `lasso {...}`, `remix {enabled, scripts<=20}`, `routineSuggestion`, `memory {episodicRetentionDays, dreamingMinHours, dreamingMinSessions}`, `compaction`, `retry`, `analytics.enabled`, `experimental {sync, kakaotalk, onboarding}`, `completionSound`, task notification flags, `cli.ephemeral` (true).

### 4.5 Private extension APIs [SRC]

`chrome.asideAccount` (getProfiles, getProfileContext, openProfileRoute, signDaemonAuthChallenge), `asideOmnibox` (about 50 members), `asideAdblock` (about 30), `asideBrowserImport`, `asideNotification` (requestPermission, revokePermission), `asideBrowserPreferences` (getTabStyle). Permissions: `at.studio.Aside.ext.private.{account, adblock, browser-import, notification, omnibox, pref-get-set, capture-tab-without-userinteraction, launch-extension}`.

---

## 5. Assessment

### 5.1 Best in class (for this product category)

1. **Credential safety for agents**: opaque refs, fill-by-ref, sender allowlist, external-manager fallback order (record 061).
2. **Authenticated local CDP** and challenge-signed daemon auth: a small, standard mechanism that removes the main local-attack surface of agent browsers (records 050, 051).
3. **Permission model as data**: modes plus rules with browser read/modify/download classes and URL globs, enforced at API hooks (records 056, 057).
4. **Approval design**: typed review cards, native popover approvals, Final confirm discipline (records 015, 058).
5. **Snapshot ergonomics**: refs, frame-aware, diff by default, scoped selectors, redaction (record 054).
6. **Careful micro-engineering** in the shell: first-frame new tab, single-owner tab preview streams, stale-context takeover on the bridge, idempotent reconnect (records 009, 010, 047).
7. **Spend control for agents** (record 059) and **activity capture with redaction-first design** (records 040, 041).

### 5.2 Mediocre or risky

- **Prompt-injection defence is heuristic** and switched off for high-capability models; no action-level policy (for example, no hard block on cross-domain form submits). (record 060)
- **Cookie-based site APIs and a CAPTCHA solver** raise terms-of-service and enterprise-compliance risk (records 067, 068).
- **Privacy leaks by design**: tab titles and URLs go to the cloud for Tidy; the remote-control relay carries session content; Google services strings remain (spell-check, preload, Safe Browsing) from upstream.
- **Public loopback paths** (`/session/for-chrome/*`) expose chat titles to any local process; the Host-header check only prevents DNS rebinding.
- **Locator click and fill paths** were not seen calling the browser access hook in the call sites I extracted, so in-page changes may rely on tool-level rules only (record 057, medium).
- **Many Google features are present from upstream** (Gemini, Glic, Spark auto browse strings); which are enabled is unknown, so some of the surface may be dormant (records 092, 093).
- **REPL as the only browser tool** makes policy enforcement depend on every Page method calling the hook; a structured tool set (as Verevon's `browser.act`) is easier to audit.

### 5.3 Limits

Single-user, single-machine, Windows-first, desktop browser. No server-side browsing fleet, no proxy pool, no crawl scale, no org-level policy engine beyond managed-profile strings.

---

## 6. What Verevon should copy, adapt or avoid

Constraints that shape every recommendation (from the Verevon baseline and CLAUDE.md [DOC]): the Model proposes browser actions and Quarry executes or rejects them; any in-app browser stays a remote leased Chromium streamed as frames, not a webview the SPA drives; no cross-plane DB access; ZDR must propagate; Space access never widens ACLs. Aside's topology (a local agent driving the user's own browser) is therefore **not** adoptable as is. The protocol-level ideas below are.

### 6.1 Copy or adapt (priority order)

| # | Idea | Why it matters for Verevon | Where it lands | Effort | Records |
|---|---|---|---|---|---|
| 1 | **Action classes read / modify / download with URL globs, deny over ask over allow** | Verevon has `allowed_domains` plus single-use broker grants that nobody issues; this gives a data-driven policy schema for grants and approvals | BrowserBroker grant model, Quarry action admission, Model Plane approvals | M | 056, 057 |
| 2 | **Typed approval cards and an approvals surface outside the chat** | BrowserChrome shows a generic approve/reject card; destination-shaped previews make approvals meaningful | Frontend Plane (cards), Application Plane (notifications), Model Plane (request_action_confirmation equivalent) | M | 015, 058 |
| 3 | **Opaque secret references** for passwords, cards and tokens | Keeps secrets out of prompts, logs and evidence; fits ZDR | Control or Integration Plane credential service; Model Plane tool args use refs only | M | 061 |
| 4 | **Untrusted-output envelope plus tag neutralisation**, conditional on model tier | Verevon screens provenance classes but has no model-side envelope on the browser path | execution-core tool result post-processor | S | 060 |
| 5 | **Snapshot diff by default, frame-aware refs, scoped snapshots, stale-ref error** | Quarry already has AX refs; diff output shrinks tokens and speeds loops | Quarry `observe` response, execution-core | S to M | 054 |
| 6 | **Session-to-tab binding and presence signals** | Chat has no link to the page it is about; presence suppresses duplicate notifications | Gateway session model, Application Plane | S | 012, 063, 097 |
| 7 | **Shell basics with high user value**: bookmarks, downloads list, reader view, permission prompts, a tab strip with groups, recently closed | Baseline lists these as absent; the preview is only a scrape panel | SolidJS BrowserChrome (UI only), Quarry events | M to L | 003, 006, 008, 023, 024 |
| 8 | **Page-context attachments**: element-level selection (selector, role, href), region screenshot, tab mention | Gives the agent precise grounding without coordinates | BrowserChrome and chat composer | M | 017, 018 |
| 9 | **Capture-time redaction before durable storage** (patterns, Luhn, sensitive fields) | Evidence and receipts are stored; ZDR-adjacent hygiene | Quarry artifact pipeline, Data Plane ingestion | S | 041 |
| 10 | **Agent incognito semantics** as the user-visible face of ZDR (no memory or history tools, hidden from lists, auto-expiry) | ZDR is currently a toggle; this explains it | Frontend, Model Plane run config | S | 043 |
| 11 | **Agent tab hygiene**: separate agent group, never close the active tab, idle auto-close, dedupe by normalised URL | Prevents the classic "agent closed my tab" and tab sprawl in the preview | Quarry tab manager, BrowserChrome | S | 048, 062 |
| 12 | **Spend control policy** (ask, auto under X, never; caps) if agents ever buy | Required before any checkout automation | Model Plane policy, Control Plane limits | M | 059 |
| 13 | **Event sources from the web**: web push, webhooks as routine triggers | Complements Quarry monitoring | Ingestion plane triggers | M | 069, 070 |
| 14 | **Expose browser tools as MCP** for external coding agents, with credentials staying inside | A distribution channel and an Integration Plane story | Integration Plane MCP gateway | M to L | 071, 072 |
| 15 | **Authenticated debugging endpoints** if Quarry ever exposes CDP or devtools outside the pool | Avoid open ports; challenge-response with short tokens | Quarry edge | S | 050 |
| 16 | **Tab Tidy style AI organisation**, local clustering first, cloud only names groups | Cheap and privacy-reviewable | Frontend Plane with gateway call | S | 007 |
| 17 | **Idempotency class per tool** for crash recovery | Durable runs in execution-core | Model Plane | S | 089 |
| 18 | **Playbooks as markdown keyed by URL pattern** | Human and agent editable site knowledge for Quarry presets | Quarry presets | S | 066 |

### 6.2 Avoid

- **Local agent that drives the user's own browser as the primary architecture.** It conflicts with the plane rules and with ZDR and multi-tenancy; Verevon's equivalent is a leased Quarry browser the user can watch and take over (which it already has).
- **Cookie-based private site APIs** (record 067) and **CAPTCHA solving** (record 068): compliance risk; prefer OAuth connectors and human takeover.
- **REPL-only control**: keep a typed tool surface where each verb is classified and policy-checked.
- **Sending tab lists or page text to a vendor cloud by default** (Tidy planner, remote relay). If copied, route through the Verevon gateway with ZDR checks.
- **Building a vault.** Integrate existing managers behind a provider interface.
- **Chasing upstream-only shell features** (omnibox AIM plumbing, Glic): large cost, little value for a web workspace.

### 6.3 Gaps Aside fills that the Verevon baseline lists as broken or missing

| Verevon gap (baseline) | Aside pattern |
|---|---|
| AI runs blocked by missing broker grants; MCP `browser.act` unreachable | Policy as data with mode defaults, so a missing grant means ask the user, not fail (records 056, 057) |
| UI vocabulary (selector, coordinates) differs from Model Plane AX-ref vocabulary | One ref-based vocabulary across UI, agent and capture (record 054) |
| Agent uses a hidden lease, no true co-browsing | Agent tabs visible in the user's strip with a takeover banner (record 062) |
| Annotate or comment is a stub | Response annotations and element-level selections (record 017) |
| No agent cursor or overlay | Aside shows none in JS either; do not prioritise |
| Replay and recording absent | Screencast to ffmpeg recording and session replay hooks (record 086) |
| Fleet console stub | Sub-agent profiles and tool derivation by session kind (records 065, 088) |

---

## 7. Open questions and unverified points

1. Which upstream Gemini, Glic and Spark features are enabled in a default Aside build. The strings and WebUI routes ship; flags were not enumerated.
2. Where the payment limits and `beforePaying` rules are enforced (daemon hook, tool, or prompt only). Only schema and UI were read.
3. Whether Locator `click` and `fill` call the `browser.access` hook. Only goto, history navigation, openTab, popup, autofill unlock, content, screenshot, snapshot and download were seen.
4. The exact crypto for the installation-signed CDP challenge, and where the installation key lives.
5. Native takeover mechanics (input blocking, split layout) are in C++ and were inferred from symbol names.
6. Who renders the "agent cursor" shown in marketing imagery, if anyone.
7. The ffmpeg binary for video recording (bundled with the agent runtime, or required on PATH).
8. Why `https://api.anthropic.com/*` is in the agent manager's `host_permissions` (no code or chunk uses it).
9. The sync protocol (`browser-sync.asidehq.com`) and whether the daemon flag `experimental.sync` gates browser sync or only agent file sync.
10. Vault sync and the sharing key exchange were not read (large 1.8 MB bundle).
11. The `/session/for-chrome` public paths: confirm the account scoping (`requestAccountMiddleware`) is sufficient for local multi-user machines.

## 8. Method notes and limits

- The AM and PM bundles are Vite or Rolldown outputs, beautified, with React, tRPC, Zod, Framer Motion, Shiki and Mermaid; feature names come from message types, endpoint literals, settings schemas and shipped UI strings. I did not execute any code.
- Daemon facts come from byte-offset reads of the embedded payload; the offsets in the JSON let anyone re-open the same passage.
- Pak strings are mostly upstream Chromium text with the brand replaced. Where a record rests only on strings, its confidence is medium and the note says so.
- Marketing images shipped in `AM/assets` (lasso, mini popup, payment use) were viewed to confirm UI shape; they are not treated as evidence of behaviour.
- Secrets handling: two manifests contain public keys (not recorded); `resources/accessibility/reading_mode_gdocs_helper_manifest.json` contains a private key field and was not read in detail; one Google API-key-shaped string in the daemon payload was counted but not recorded.
