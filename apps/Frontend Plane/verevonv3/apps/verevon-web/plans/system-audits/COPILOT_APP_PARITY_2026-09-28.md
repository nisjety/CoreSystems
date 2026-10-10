# Microsoft Copilot app: how it works and what Verevon should match (2026-09-28)

Sources:
- Microsoft's official documentation (learn.microsoft.com, support.microsoft.com, blogs.windows.com, blogs.microsoft.com);
- a read-only inspection of the Copilot app installed on the owner's Windows 11 PC. Nothing was launched or signed into, and no credential or token stores were read;
- the Verevon repository.

Two fact-check passes corrected the drafts, and their corrections are folded in here. The second pass (2026-09-28) checked every claim against file:line or a dated Microsoft page, and its verdicts are listed in section 7. Anything not confirmed is marked UNVERIFIED.

**Owner priority (2026-09-28):** the web app comes first. From Copilot, the owner wants:
- connectors;
- Office documents;
- data protection;
- the agentic stack, as Verevon controlling Chrome and working with Excel on the web.

Windows-shell integration (sections 2 and 5, Phase 2a) is parked until then. Chat-page work is tracked in `CHAT_PAGE_GAP_PLAN_2026-09-28.md`.

## 1. How the Copilot app works

**It is three Microsoft-signed layers wired together, not one app.** On this PC:
- **A Win32 app** at `C:\Program Files (x86)\Microsoft\Copilot` (v154.0.4258.37). It is built on WebView2 and WinUI 3, is updated through the same pipeline as Edge and Teams, and has a LocalSystem elevation service.
- **The in-box `Microsoft.MicrosoftOfficeHub` package** (display name "Copilot"). This is what Windows itself talks to. It owns:
  - the Start tile;
  - the physical Copilot key (`com.microsoft.windows.copilotkeyprovider`);
  - the taskbar "Ask Copilot" chat slot (`com.microsoft.windowsai.chat`);
  - an "Ask Microsoft 365 Copilot" File Explorer verb on about 40 file types;
  - a Share target for any file, text or image;
  - an auto-start task;
  - the URI schemes `ms-copilot:`, `ms-officeapp:` and `ms-getoffice:`;
  - a web link handler for `m365.cloud.microsoft`.
  
  Its COM servers launch `undocked_copilot\copilotapphost.exe` from the Win32 folder.
- **Shared Microsoft identity:** OneAuth/WAM identity helpers, the same single sign-on broker Outlook, Teams and Office use. So one sign-in covers them all.

**What was not on this PC:** a populated Windows App Actions file (the manifest references `ActionRegistration.json`, but the file is absent), MCP server files, and `odr.exe`. Microsoft's MCP-on-Windows platform is documented but still pre-release.

**How it reaches Microsoft apps and data:**
- **Connectors:** OneDrive, Outlook mail, calendar and contacts, Gmail, Google Drive, Google Calendar and Google Contacts.
  - They are opt-in from the app's Settings → Connectors, and each one goes through that provider's own OAuth consent.
  - Microsoft states connected data keeps the user's existing permissions and is not used for training.
  - The exact Graph scopes for the consumer app are not published (UNVERIFIED).
  - Source: blogs.windows.com/windows-insider/2025/10/09/copilot-on-windows-connectors-and-document-creation.
- **Office documents:** a chat can be exported to `.docx`, `.xlsx`, `.pptx` or PDF. Answers of **600 characters or more** get a default export button (same source).
  - Inside Word, Excel, PowerPoint, Outlook, OneNote and Teams, Copilot is built in. That is Microsoft 365 Copilot on a work account and a paid licence.
- **Windows shell:**
  - an opt-in "Hey Copilot" wake word, detected on the device and English only;
  - Copilot Vision, a live "share this window with Copilot" from the taskbar;
  - Click to Do and Recall, which need a Copilot+ PC with a 40+ TOPS NPU.
  - File Explorer "Ask Copilot" about a file, which needs only a Microsoft 365 Personal/Family/Premium subscription or a Copilot-licensed work account, on any Windows 10/11 PC.
- **Agentic stack:**
  - Copilot Actions, which drive apps in an isolated "Agent Workspace" (a separate Windows session under a low-privilege account), off by default. It is not offered in the EEA; reporting points to the Digital Markets Act.
  - Edge Copilot Mode.
  - Microsoft 365 Copilot Cowork, metered through Copilot Credits.
- **Models:** multi-vendor. Anthropic has been a Microsoft 365 Copilot subprocessor since **7 January 2026**.
  - Anthropic models are on by default for most commercial tenants, but **off ("No users") by default in the EU, EFTA and UK**.
  - Separately, a narrower "Copilot in Microsoft 365 apps with Anthropic models" toggle (Word, Excel, PowerPoint only) is on by default in those regions, for tenants created after 25 March 2026.
  - Sources: learn.microsoft.com/microsoft-365/copilot/connect-to-ai-subprocessor (2026-09-18) and /copilot-anthropic-apps (2026-09-03).
- **Data protection:** Enterprise Data Protection and the EU Data Boundary apply to Microsoft 365 Copilot and Copilot Chat. They are architected and audited, not just policy statements.
  - **Anthropic models are excluded from the EU Data Boundary** (learn.microsoft.com/microsoft-365/copilot/enterprise-data-protection, footnote 3).
  - Data Guardian and External Key Management are general Microsoft 365 / Azure sovereign-cloud controls, not Copilot-specific features.
  - Copilot's own sovereignty story is an in-country processing and residency programme.
- **How third parties plug in:**
  - declarative agents calling MCP servers (generally available) or OpenAPI APIs;
  - Copilot (Graph) connectors, synced or federated;
  - Copilot Studio and the Agent Store. **Each customer's tenant admin must approve an agent** before its users see it.
  - On Windows:
    - MCP servers registered with the On-Device Agent Registry (pre-release);
    - App Actions;
    - **Agent Launchers**, which put an agent into taskbar "Ask Copilot" (Insider Dev/Beta preview since build 26220.7523, Dec 2025);
    - the **Copilot key provider** extension;
    - Share targets, File Explorer context menus and URI handlers.

## 2. What Verevon can plug into, and what Microsoft reserves

| Extension point | Open to Verevon? |
|---|---|
| Microsoft Graph with delegated OAuth (mail, calendar, OneDrive, SharePoint, Teams) | Yes. Already in the catalog (`integration-corev2/internal/providers/catalog.go:214-298`). **Consent caveat, below.** |
| MCP server that Microsoft 365 Copilot declarative agents, or Windows MCP, can call | Yes. Declarative-agent MCP plugins are generally available; Windows MCP is pre-release. |
| Microsoft 365 declarative agent / Copilot connector | Yes, but each tenant's admin approves it separately. |
| Packaged Windows app: Share target, File Explorer verb, `verevon:` URI, App Actions | Yes. These are documented MSIX extensions, and Store distribution re-signs the package, taking about 3 days to certify. |
| The Copilot key (`com.microsoft.windows.copilotkeyprovider`) | **Yes** (corrected). Any signed MSIX app can declare the extension and appear in Settings → "Customize the Copilot key". It supports single tap and press-and-hold / push-to-talk through URI activation, since build 22621 (learn.microsoft.com/windows/apps/develop/windows-integration/microsoft-copilot-key-provider). Whether a PWABuilder-packaged PWA can do it is UNVERIFIED. |
| Taskbar "Ask Copilot" | **Preview route** (corrected). Agent Launchers (`com.microsoft.windows.ai.agentInfo` or `odr.exe`) make a registered agent invocable from Ask Copilot. It is Insider Dev/Beta only, not GA (learn.microsoft.com/windows/ai/agent-launchers). The in-box `com.microsoft.windowsai.chat` slot itself stays Microsoft's. |
| Preinstalled on every PC | No. That takes an OEM deal. |

**Graph consent caveat (corrected).** Per the Graph permissions reference (2026-09-14), none of the delegated permissions Verevon uses needs admin consent in the per-permission table: Calendars.ReadWrite, Mail.Send, Files.ReadWrite.All and **Sites.ReadWrite.All**. Only app-only Sites.ReadWrite.All does. The tenant's consent **policy** is a different matter:
- The default for new tenants is "Let Microsoft manage your consent settings" (`microsoft-user-default-recommended`).
- Under it, users cannot self-consent to Calendars.ReadWrite, Files.ReadWrite.All, Sites.ReadWrite.All or Sites.Read.All for any app.
- Mail.Send is additionally blocked for unverified multi-tenant publishers by risk-based step-up.

So Verevon needs **Microsoft publisher verification** plus an **admin-consent link** (`/adminconsent`) for business tenants, and an in-app "ask your admin" fallback (learn.microsoft.com/entra/identity/enterprise-apps/manage-app-consent-policies, updated 2026-08-28).

## 3. Parity

| Capability | Copilot | Verevon today | Gap |
|---|---|---|---|
| Outlook mail, calendar, OneDrive, SharePoint, Teams | Yes | **Have:** catalog bundles and OAuth scopes (`catalog.go:237,261-298`); governed write tools (`mail.send`, …) with approvals; Reads/Writes badges shown before Connect (`WorkspaceSettingsPage.tsx:1795-1825`) | Calendar is read-only (`Calendars.Read`); the badges list the provider's whole catalogue, not the bundle actually requested; no publisher verification or admin-consent flow |
| Gmail, Google Drive, Google Calendar | Yes | **Have** (`catalog.go:404-492`) | Small |
| Office export from chat | Yes, with a one-click button | **Partial:** `.docx`/`.xlsx`/`.pptx`/`.pdf` come only as code-interpreter output (`execution-core/src/code_interpreter.rs:279-285`) | Chat-native export button and templates |
| MCP client (use other MCP servers) | Yes (pre-release on Windows) | **Have:** Streamable HTTP and stdio (`model-gateway/src/mcp_http.rs`, `mcp_jsonrpc.rs`), registry `/v1/mcp/servers`, already used with Visma Net | At parity |
| MCP **server** (others call Verevon) | Yes (Windows and Microsoft 365 as platform) | **Missing:** execution-core "has no MCP registry of its own" (`execution-core/src/mcp_gateway.rs:3`) | **Largest structural gap** |
| Windows presence (Share target, Explorer verb, URI, auto-start) | Yes | **Missing:** no packaged desktop app or PWA manifest | Greenfield |
| Live screen vision | Yes (Copilot Vision) | **Partial:** paste, upload, or a one-shot screenshot through `getDisplayMedia` that stops after one frame (`DashboardComposer.tsx:1691-1724`) | Live share is an owner decision (privacy) |
| Voice | Wake word (English only) | **Partial by design:** governed dictation and read-aloud, no always-on wake word | Deliberate privacy choice |
| Agentic actions | Agent Workspace, off by default, not in the EEA | **Have:** browser broker, sandbox manager, and per-action danger or disclosure approvals (`execution-core/src/permission/mod.rs`) | At parity, and more granular |
| Proof of what was done | Audit logs | **Have:** receipts, source-bound review receipts and Ed25519-attested effectful writes | Verevon strength |
| Cost visibility | Copilot Credits and an admin cost dashboard | **Have:** a per-org cost ledger | At parity, not a win |
| Data protection | Enterprise Data Protection, EU Data Boundary (Anthropic excluded), sovereign controls | **Have:**<br>- ZDR attestation built per provider deployment (`inference-core/src/provider/zdr.rs:1-71`), enforced on every request (`grpc.rs:176-187`)<br>- an enforced residency and privacy-tier gate, `Residency {Global, Eu, Norway}` and `PrivacyTier {…, EuResident, ZdrContractual, Sovereign}` (`provider/mod.rs:144,489-591`), which denies by default (`fallback.rs:467-489`) | No customer-managed keys (BYOK/KMS), no self-host offer, no audited certification; residency is not shown to users |
| Model choice | Multi-vendor (OpenAI, Anthropic) | Multi-provider routing, including the Codex subscription | At parity |
| Norwegian quality | English-only wake word, general-purpose | Norwegian-first product, source-checked Norwegian drafts | Verevon strength |

## 4. Where Verevon wins

- **Governed, provable work:**
  - every business action is a named, audited tool with its own approval rule;
  - drafts are checked against their sources, with a receipt;
  - effectful writes are attested.
  
  Copilot documents monitoring, not this.
- **Draft-first safety for business tasks:** customer replies, reports, campaigns and plans stay drafts until approved. There are no silent sends.
- **Norwegian and EEA fit:** Norwegian-first quality. Governed actions and approvals already available in the EEA, where Copilot Actions is not.
- **Works inside Microsoft rather than against it:** as an MCP server and declarative agent, Verevon can appear inside Microsoft 365 Copilot and keep its own governance.

Claim with care:
- **Say:** Verevon enforces residency (EU or Norway) per request, and ZDR.
- **Don't claim yet:** customer-managed keys, self-hosting or audited sovereignty. None of them is evidenced in the repository.
- **Fair contrast:** Microsoft excludes Anthropic models from its own EU Data Boundary.

## 5. Plan

**Phase 0 (days):**
- An "Export to Word / Excel / PowerPoint / PDF" button on chat answers and artifacts, reusing the existing generator. (S)
- A plain-language disclosure of connected-data and ZDR handling, in the product and in a subprocessor list. Verevon must disclose its own model providers too. (S)
- A calendar write action (`Calendars.ReadWrite`, delegated), behind the existing approval gate. (S) Under the default Entra consent policy, business users need admin consent for it. Ship it together with the admin-consent path in Phase 1.

**Phase 1 (weeks):**
- **Verevon as an MCP server:** expose governed Verevon tools (drafts, reports, plans, receipts, approvals). (L)
  - Every external call goes through the existing permission gate, approvals and receipts.
  - Treat tool descriptions as signed and versioned, and detect changes (a defence against tool poisoning).
  - Scope by org and rate-limit calls.
- **Per-service consent screens** like Copilot's, for Microsoft and Google, scoped to the bundle actually requested. (M)
  - **Microsoft publisher verification** for the Verevon app registration.
  - An **admin-consent flow** with an "ask your admin" fallback. The default tenant policy blocks user self-consent to Calendars.ReadWrite, Files.ReadWrite.All and Sites.ReadWrite.All.

**Phase 2a (buildable now):**
- A packaged Windows companion distributed through the Microsoft Store, with:
  - a Share target;
  - a File Explorer "Ask Verevon" verb;
  - a `verevon:` URI;
  - an optional auto-start;
  - **the Copilot key provider** extension, so the user can set the Copilot key to open Verevon, with push-to-talk for voice;
  - an **Agent Launcher** registration for taskbar "Ask Copilot", once it leaves preview;
  - App Actions once that schema is public and stable.
- It is a thin client over the existing web app. (M)

**Phase 2b:**
- A Microsoft 365 Copilot declarative agent built on the Phase-1 MCP server. (M)
- Budget for each customer's admin approval in the sales cycle.

**Not feasible:** the in-box taskbar chat slot itself, and OEM preinstallation. Both the Copilot key and "Ask Copilot" (through Agent Launchers) are reachable; see section 2.

**Risks to design in from the start:**
- **MCP security:** tool poisoning and prompt injection through tool metadata; Windows contains each server for this reason.
- **Screen and memory features:** Copilot's Recall backlash shows any capture feature must be opt-in, protected by re-authentication, and retention-bounded.
- **EEA agentic actions:** these face the same regulatory questions Microsoft has.

## 6. Owner decisions

1. Build **Verevon as an MCP server** (Phase 1), the prerequisite for everything inside Microsoft 365?
2. **Windows presence:** a packaged Store companion (recommended), a PWA only, or skip it for now?
3. **Microsoft 365 Copilot agent:** pursue it, knowing each customer's admin must approve it?
4. **Live screen vision:** keep paste-image only, or build an opt-in live share?
5. **Sovereignty and residency:** productize it (Norway/EEA hosting, key management) before any claim is made?

## 7. Verification ledger (2026-09-28)

One skeptic agent per claim group tried to refute each claim. Repo claims were checked by reading files, Microsoft claims by fetching the page. The full evidence is kept in the session scratchpad.

| Id | Verdict | Claim | Source |
|---|---|---|---|
| R1 | confirmed | catalog.go lines ~214-298 define Microsoft Graph bundles (mail, calendar, OneDrive, SharePoint, Teams). | apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:212-302 (read directly) |
| R2 | confirmed | catalog.go:237 requests Sites.ReadWrite.All. | apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:237 |
| R3 | confirmed | Calendar is read-only: only Calendars.Read, no Calendars.ReadWrite anywhere in the catalog. | apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go (grep 'Calendars\.' over the full file, one hit at line 271) |
| R4 | confirmed | Graph scopes include Files.ReadWrite.All and Mail.Send. | apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:237,264 |
| R5 | confirmed | catalog.go ~404-492 defines Gmail, Google Drive and Google Calendar. | apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:402-496 (read directly) |
| R6 | confirmed | Governed write tools such as mail.send exist and require an approval before executing. | apps/Model Plane/rust/services/execution-core/src/integration_tools.rs:101,109,594-608 |
| R7 | refuted | There is no per-service consent screen in the frontend that lists what each connector can read/write before OAuth. | apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx:91-97,1546,1571-1574,1789-1826,2018-2102,2212-2244 |
| R8 | confirmed | execution-core/src/code_interpreter.rs lines ~279-285 map docx/xlsx/pptx/pdf MIME types, and Office files (.docx/.xlsx/.pptx/.pdf) are produced only as code-int | apps/Model Plane/rust/services/execution-core/src/code_interpreter.rs:274-284; apps/Model Plane/rust/services/model-gate |
| R9 | confirmed | model-gateway src/mcp_http.rs is an MCP Streamable HTTP CLIENT transport and src/mcp_jsonrpc.rs covers stdio/JSON-RPC client transport; model-gateway exposes re | apps/Model Plane/rust/services/model-gateway/src/mcp_http.rs:1-26; apps/Model Plane/rust/services/model-gateway/src/mcp_ |
| R10 | confirmed | Verevon is already used with Visma Net via MCP. | commit 03913219122bcbe2e2df701e7d31cdd62b7395cd (2026-07-28); commit 3ddc12a01793e29825ee9d84c3923335fbe3d380 (2026-08-2 |
| R11 | confirmed | execution-core src/mcp_gateway.rs line ~3 says execution-core 'has no MCP registry of its own'. | apps/Model Plane/rust/services/execution-core/src/mcp_gateway.rs:1-4 |
| R12 | confirmed | Verevon does NOT expose itself as an MCP server anywhere (no service — Go integration services or the Rust gateway verevon-gateway-rs — serves initialize/tools/ | apps/Model Plane/rust/services/model-gateway/src/{mcp_http.rs,mcp_jsonrpc.rs,mcp_oauth.rs,runtime_registries.rs,http_rou |
| R13 | confirmed | There is no packaged desktop app (no MSIX/appxmanifest, no Electron/Tauri app) and no PWA web app manifest (manifest.webmanifest / manifest.json with display st | apps/Frontend Plane/verevonv3/index.html:1-13 |
| R14 | partly | Image input is paste/upload only; there is no live screen or window sharing (getDisplayMedia) feature. | apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:1691-1724,2370,2837-2838,3384-3386,4372-4382 |
| R15 | confirmed | Voice is governed dictation (speech-to-text) and read-aloud (text-to-speech) only; there is no always-on wake word. | apps/Frontend Plane/verevonv3/src/features/voice/voice-recorder.ts:1-9 |
| R16 | confirmed | The product is Norwegian-first (UI strings default nb/no locale). | apps/Frontend Plane/verevonv3/src/shared/i18n/locales.ts:1-19 |
| R17 | confirmed | execution-core src/permission/mod.rs implements per-action danger or disclosure approvals. | apps/Model Plane/rust/services/execution-core/src/permission/mod.rs:41-107,159-161 |
| R18 | confirmed | There is a browser broker and a sandbox manager for agentic actions. | apps/Model Plane/go/services/browser-broker/ (directory) |
| R19 | confirmed | Turns produce receipts, including source-bound review receipts for drafts (model-gateway source_validation / receipts). | apps/Model Plane/rust/services/model-gateway/src/source_validation.rs:11-12,999-1050,1663 |
| R20 | confirmed | Effectful writes are attested with Ed25519 signatures (find the signing code and what exactly is signed). | apps/Model Plane/rust/services/execution-core/src/attestation.rs:1-9,49,74-95,100-107,184,226,234 |
| R21 | confirmed | There is a per-organisation cost ledger (cost-core). | apps/Model Plane/go/services/cost-core/internal/ledger/ledger.go:1-9,81,113,323 |
| R22 | partly | inference-core src/provider/zdr.rs implements per-request zero-data-retention attestation. | apps/Model Plane/rust/services/inference-core/src/provider/zdr.rs:1-71 |
| R23 | refuted | Data residency or sovereignty is only GDPR metadata (no region pinning, customer-managed keys or self-hosting option is evidenced). | apps/Model Plane/rust/services/inference-core/src/provider/mod.rs:144,489-509,528,542,570-591,639-655 |
| R24 | confirmed | Multi-provider model routing exists, including the openai-codex-subscription provider. | apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs:558-666,693,1080,2290 |
| W1 | confirmed | Delegated Calendars.ReadWrite does not require admin consent. | https://learn.microsoft.com/en-us/graph/permissions-reference |
| W2 | confirmed | Delegated Mail.Send does not require admin consent. | https://learn.microsoft.com/en-us/graph/permissions-reference |
| W3 | confirmed | Delegated Files.ReadWrite.All does not require admin consent. | https://learn.microsoft.com/en-us/graph/permissions-reference |
| W4 | refuted | Delegated Sites.ReadWrite.All DOES require admin consent. | https://learn.microsoft.com/en-us/graph/permissions-reference |
| W5 | partly | The default Entra user-consent setting for new tenants today is 'Allow user consent for apps from verified publishers, for selected permissions,' and it blocks  | https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/manage-app-consent-policies |
| W6 | confirmed | Copilot on Windows can export a chat answer to Word (.docx), Excel (.xlsx), PowerPoint (.pptx) or PDF, with an export button shown on long answers. | https://blogs.windows.com/windows-insider/2025/10/09/copilot-on-windows-connectors-and-document-creation/ |
| W7 | confirmed | Copilot on Windows has connectors for OneDrive, Outlook (mail, calendar, contacts), Gmail, Google Drive and Google Calendar, each behind OAuth consent, opt-in. | https://blogs.windows.com/windows-insider/2025/10/09/copilot-on-windows-connectors-and-document-creation/ |
| W8 | confirmed | "Hey Copilot" wake word is opt-in, detected on-device, English only. | https://support.microsoft.com/en-us/microsoft-copilot/copilot-wake-word-hey-copilot |
| W9 | confirmed | Copilot Vision lets the user share a window/screen with Copilot from the taskbar. | https://support.microsoft.com/en-us/topic/december-1-2025-kb5070311-os-builds-26200-7309-and-26100-7309-preview-5cd455bf-3291-47fa-b0bf-e5f60d0ea7af |
| W10 | confirmed | Copilot Actions on Windows run in an isolated "Agent Workspace" (separate Windows session, separate low-privilege agent account), off by default; and Copilot Ac | https://learn.microsoft.com/en-us/windows/security/book/operating-system-agentic-security |
| W11 | partly | Click to Do, Recall and File Explorer "Ask Copilot" need a Copilot+ PC with a 40+ TOPS NPU. | https://support.microsoft.com/en-au/topic/copilot-pc-hardware-requirements-35782169-6eab-4d63-a5c5-c498c3037364 |
| W12 | partly | Anthropic has been a Microsoft 365 Copilot subprocessor since 6 January 2026, and Claude models are the default in Microsoft 365 apps for the EU, EFTA and UK. | https://learn.microsoft.com/en-us/microsoft-365/copilot/connect-to-ai-subprocessor |
| W13 | confirmed | Microsoft 365 Copilot declarative agents can call MCP servers, and this is generally available (not preview). | https://learn.microsoft.com/en-us/microsoft-365/copilot/extensibility/build-mcp-plugins |
| W14 | confirmed | A tenant admin must approve/deploy a third-party agent before users in that tenant can use it (Agent Store / Integrated apps). | https://learn.microsoft.com/en-us/microsoft-365/copilot/copilot-agent-store |
| W15 | confirmed | Microsoft 365 Copilot Cowork exists and is metered through Copilot Credits; there is an admin cost-management view for Copilot Credits. | https://learn.microsoft.com/en-us/microsoft-365/copilot/usage-based-billing-overview-copilot-credits |
| W16 | partly | Enterprise Data Protection, the EU Data Boundary, and a Microsoft Sovereign Cloud offering (Data Guardian, External Key Management) apply to Microsoft 365 Copil | https://learn.microsoft.com/en-us/microsoft-365/copilot/enterprise-data-protection |
| W17 | confirmed | Microsoft 365 Copilot price for business/enterprise (per user per month) as of 2026. | https://blogs.microsoft.com/blog/2023/07/18/furthering-our-ai-ambitions-announcing-bing-chat-enterprise-and-microsoft-365-copilot-pricing/ |
| W18 | refuted | The Copilot hardware key has no public third-party route (report: COPILOT_APP_PARITY_2026-09-28.md:56 "No public third-party route found. Treat as Microsoft-res | https://learn.microsoft.com/en-us/windows/apps/develop/windows-integration/microsoft-copilot-key-provider |
| W19 | refuted | The taskbar "Ask Copilot"/AI chat slot (com.microsoft.windowsai.chat) is Microsoft-reserved, with no documented third-party route (same report row, COPILOT_APP_ | https://blogs.windows.com/windows-insider/2025/12/19/announcing-windows-11-insider-preview-build-26220-7522-dev-beta-channels/ |
| W20 | confirmed | Status (preview/GA) of MCP on Windows / On-Device Agent Registry (odr.exe) and Windows App Actions, and how a third party registers an MCP server or App Action  | https://learn.microsoft.com/en-us/windows/ai/mcp/overview |
| W21 | confirmed | Share target, File Explorer context-menu verb, and a custom URI protocol are documented MSIX package extensions available to third parties; Microsoft Store re-s | https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap-sharetarget |
| W22 | partly | Microsoft documents a PWA route (web app manifest share_target, protocol_handlers, file_handlers) that Edge/Windows honour for installed PWAs, and PWABuilder ca | https://learn.microsoft.com/en-us/microsoft-edge/progressive-web-apps/how-to/share |
