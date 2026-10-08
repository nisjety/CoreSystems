# App registrations: Microsoft Entra and Google (runbook and checklist)

Task G12 of the chat gap-closure plan. Written on 2026-10-01 from the repository and from public vendor documentation read that day. Most steps here can only be done by the owner, in the owner's own Microsoft, Google and Partner Center accounts. **This document records no fact about the owner's tenants, projects or accounts.** Every statement about the repository carries a citation that `docs/compliance/evidence/tools/check-citations.mjs` can re-open (run it with `--no-header-rule --no-evidence-rule`, because this is a runbook and not an evidence document). Every statement about Microsoft or Google is marked `[vendor doc]` and is a pointer to confirm in the vendor's own console, not a promise: those documents change.

## 0. Who does what

| Step | Who | Why only them |
|---|---|---|
| Create or change an app registration, platform type, redirect URIs, permissions, credentials | Owner | Needs an account in the owner's Entra tenant with the Application Administrator or Cloud Application Administrator role, and the Google Cloud project's owner or editor role. |
| Microsoft publisher verification | Owner | Needs the organisation's Microsoft Cloud Partner Program account (Partner One ID), a verified publisher domain, multifactor sign-in and a role in both Entra and Partner Center. [vendor doc] |
| Google brand and scope verification, restricted-scope security assessment | Owner | Needs the Google Cloud project, domain ownership in Search Console, a privacy policy, a demo video and, for restricted scopes, an assessor. [vendor doc] |
| Create credentials (client secrets, certificates) and put them in the service environment | Owner | Secrets are never created, shown or stored by an agent, and never written in chat, a document or a log. Only the expiry date is recorded. |
| Provide the live URLs of the privacy policy, terms of service, home page and support page | Owner | They are public statements of the company. The repository has none to cite (section 1.4). |
| Test the consent screen on a business tenant and on Google | Owner | Needs a non-admin member and an admin of a business tenant, and a real Google account. An agent cannot create accounts or enter credentials. |
| Decide D3b (section 3) | Owner | It changes who can sign in. |
| Code: startup warnings for missing registration facts, the admin-consent route and the ask-your-admin card | Engineering | Not done by this document. `ValidateProvider` checks only that an id (and a secret) is present (section 1.4); the admin-consent route and card are task G11. |

## 1. What the repository says

All facts in this section were read on 2026-10-01. Several of the files are edited by other work, so a line number can move; the anchors in the citations catch that.

### 1.1 Microsoft: two registrations, and a fallback between them

| | Sign-in registration (auth-core) | Connector registration (integration-corev2) |
|---|---|---|
| Used for | Signing a person in with a Microsoft account; its token is handed to the connector service after each sign-in. | The Microsoft 365 connection (SharePoint and OneDrive, Teams, Outlook, calendar). |
| Client id variable | `MICROSOFT_CLIENT_ID` and `MICROSOFT_CLIENT_SECRET`; the block is switched on only when both are set. `apps/Control Plane/auth-core/src/auth/auth.ts:1429 @"...(process.env.MICROSOFT_CLIENT_ID && process.env.MICROSOFT_CLIENT_SECRET"` | `AZURE_CLIENT_ID` (falls back to `MICROSOFT_CLIENT_ID`), and `AZURE_CLIENT_SECRET` only in confidential mode. `apps/Ingestion Plane/integration-corev2/internal/config/config.go:370 @"AZURE_CLIENT_ID"` |
| Audience (tenant) | `MICROSOFT_TENANT_ID`, default `common`, with the code comment "multi-tenant". `apps/Control Plane/auth-core/src/auth/auth.ts:1434 @"tenantId: process.env.MICROSOFT_TENANT_ID \|\| 'common'"` | `AZURE_TENANT_ID`, default `common`. `apps/Ingestion Plane/integration-corev2/internal/config/config.go:252 @"AZURE_TENANT_ID"` |
| Client type | Confidential (a client secret). | `MICROSOFT_CLIENT_AUTH_MODE`, default `public`: PKCE, no secret, and an `Origin` header on the token request. `confidential` needs `AZURE_CLIENT_SECRET`. `apps/Ingestion Plane/integration-corev2/internal/config/config.go:372 @"MicrosoftClientAuthMode:   normalizeMicrosoftClientAuthMode"`<br>`apps/Ingestion Plane/integration-corev2/internal/oauth/microsoft.go:186-195 @"func (c *MicrosoftClient) setTokenOrigin"`<br>`apps/Ingestion Plane/integration-corev2/internal/config/config.go:632 @"AZURE_CLIENT_SECRET is required for Microsoft OAuth"` |
| Redirect URI | Better Auth's callback under the public origin in `BETTER_AUTH_URL`. The repository builds the same shape for Vipps and Okta (`.../api/auth/callback/<provider>`); for Microsoft it is the library's default and the repository's own SSO guide says `/api/auth/callback/microsoft`. Confirm what the running service sends. `apps/Control Plane/auth-core/src/auth/auth.ts:855 @"api/auth/callback/vipps"`<br>`apps/Control Plane/docs/SSO_SETUP_GUIDE.md:67 @"api/auth/callback/microsoft"` | `<INTEGRATION_PUBLIC_BASE_URL>/oauth/callback/microsoft`. The default base is `http://localhost:3026`; the production value is not in the repository. `apps/Ingestion Plane/integration-corev2/internal/oauth/service.go:528 @"/oauth/callback/"`<br>`apps/Ingestion Plane/integration-corev2/.env.example:120 @"# Redirect URL: http://localhost:3026/oauth/callback/microsoft"` |
| Scopes it requests | At sign-in: `openid profile email offline_access User.Read Files.Read.All Sites.Read.All Team.ReadBasic.All Channel.ReadBasic.All ChannelMessage.Read.All Chat.Read Mail.Read`. `apps/Control Plane/auth-core/src/auth/auth.ts:1457 @"'Files.Read.All',"`<br>`apps/Control Plane/auth-core/src/auth/auth.ts:1461 @"'ChannelMessage.Read.All',"` | By bundle (section 1.2). `apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:212 @"func Microsoft() Provider"` |
| Prompt | Better Auth's default. | `prompt=select_account` only. There is no admin-consent URL and no `/oauth/callback/microsoft-admin` route today: task G11 adds them. `apps/Ingestion Plane/integration-corev2/internal/oauth/clients.go:246 @"select_account"`<br>`apps/Ingestion Plane/integration-corev2/internal/api/server.go:255 @"/oauth/callback/:provider"` |

Two further facts.

- **The sign-in token feeds the connector.** After each sign-in, auth-core hands the Microsoft token to integration-corev2, which adopts it into the organisation's connection, "widening, never narrowing" its capabilities, so that the connection "refreshes itself on every login". `apps/Control Plane/auth-core/src/auth/microsoft-signin-handoff.ts:12 @"widening, never narrowing"`
- **A fallback between the two registrations.** When the connector's own refresh fails, it asks auth-core to mint a token from the sign-in registration, which holds no write scopes. The connector's code says the refresh token "belongs to auth-core's Azure app registration and can only be redeemed with that app's client secret". `apps/Ingestion Plane/integration-corev2/internal/controlplane/oauth_client.go:21 @"belongs to auth-core's Azure app registration"`

**Not in the repository:** whether the two variables name one Entra application or two (the names differ), which platform type the connector registration uses in the portal, and the production values of `BETTER_AUTH_URL` and `INTEGRATION_PUBLIC_BASE_URL`.

### 1.2 What the Microsoft connector requests today

Capabilities and the bundles that include them (catalog and the web app's connect policy):

| Capability | Scopes | In which bundles |
|---|---|---|
| `profile.read` | `openid profile email offline_access User.Read` | all |
| `sharepoint.read` | `Files.Read.All`, `Sites.Read.All` | onboarding, knowledge, full |
| `teams.read` | `Team.ReadBasic.All`, `Channel.ReadBasic.All` | onboarding, knowledge, inbox, full |
| `teams.messages.read` | `ChannelMessage.Read.All`, `Chat.Read` | inbox, full |
| `mail.read` | `Mail.Read` | inbox, full |
| `mail.send` | `Mail.Send` | inbox, full |
| `calendar.read` | `Calendars.Read` | full |
| `sharepoint.write` | `Files.ReadWrite.All`, `Sites.ReadWrite.All` | none: defined in the catalog, in no bundle |

`apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:231 @"Files.Read.All"`<br>`apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:237 @"Files.ReadWrite.All"`<br>`apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:250 @"ChannelMessage.Read.All"`

The default "Microsoft 365" connect asks for the union of the `onboarding` and `inbox` bundles, so a person who clicks Connect is asked for everything in rows one to six except `calendar.read`. `apps/Frontend Plane/verevonv3/src/shared/integrations/connect-bundles.ts:44 @"defaultSources: ['sharepoint', 'onedrive', 'outlook', 'teams'],"`

**Registration rule for the connector registration:** list delegated permissions only, and only those of the bundles the product requests. Do not add the write permissions of `sharepoint.write` until a write bundle ships, and do not add any application permission.

### 1.3 Google

| | Sign-in client (auth-core) | Connector client (integration-corev2) |
|---|---|---|
| Variables | `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`; the block is on only when both are set. `apps/Control Plane/auth-core/src/auth/auth.ts:1415 @"...(process.env.GOOGLE_CLIENT_ID && process.env.GOOGLE_CLIENT_SECRET"` | `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`; both are required. `apps/Ingestion Plane/integration-corev2/internal/config/config.go:383 @"GoogleClientID:            strings.TrimSpace"` |
| Scopes | `openid profile email`, offline access. `apps/Control Plane/auth-core/src/auth/auth.ts:1420 @"scope: ['openid', 'profile', 'email'],"` | By bundle (below), PKCE, offline access, `prompt=select_account consent`, `include_granted_scopes=true`. `apps/Ingestion Plane/integration-corev2/internal/oauth/clients.go:58 @"select_account consent"` |
| Redirect URI | `<BETTER_AUTH_URL>/api/auth/callback/google` (the repository's SSO guide gives this path). `apps/Control Plane/docs/SSO_SETUP_GUIDE.md:148 @"api/auth/callback/google"` | `<INTEGRATION_PUBLIC_BASE_URL>/oauth/callback/google`. `apps/Ingestion Plane/integration-corev2/.env.example:133 @"# Redirect URL: http://localhost:3026/oauth/callback/google"` |

**Not in the repository:** whether the two Google variables name one OAuth client or two.

Scopes the connector requests today, with the tier Google's own page gives them. The restricted tier: Google's support page lists restricted Drive and Gmail scopes (read 2026-10-01, `[vendor doc]`; the page lists more scopes than the ones named here, so read the whole page). Of the scopes that matter for this repository, these are on it: `drive`, `drive.readonly`, `drive.metadata.readonly`, `gmail.readonly`, `gmail.modify`. For every other scope, the tier is what the Cloud Console shows next to it on the consent screen's data-access page; this document does not assert it.

| Capability | Scope | Bundles | Tier |
|---|---|---|---|
| `drive.metadata` | `drive.metadata.readonly` | onboarding | restricted `[vendor doc]` |
| `drive.read` | `drive.readonly` | knowledge, full | restricted `[vendor doc]` |
| `gmail.read` | `gmail.readonly` | inbox, full | restricted `[vendor doc]` |
| `gmail.send` | `gmail.send` | inbox, full | not on the restricted list: read the tier in the console |
| `calendar.read` | `calendar.readonly` | full | not on the restricted list: read the tier in the console |
| `ads.manage` | `adwords` | ads | not on the restricted list: read the tier in the console |

`apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:421 @"drive.metadata.readonly"`<br>`apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:427 @"drive.readonly"`<br>`apps/Ingestion Plane/integration-corev2/internal/providers/catalog.go:434 @"gmail.readonly"`

Planned by other tasks of the plan and not requested today: `calendar.events`, `spreadsheets`, `contacts.readonly` (expected to be in the sensitive tier; confirm in the console). For writes to Drive prefer `drive.file`, which Google's restricted list does not name (confirm in the console), over `drive`, which is on it.

### 1.4 What the repository does not hold

- **No record** of Microsoft publisher verification, a Partner One ID, a verified publisher domain, Google OAuth consent-screen status, brand verification or a security assessment. A search of the repository for those terms finds the earlier audit that says publisher verification is needed, two older set-up guides for creating a registration (described as stale below), and nothing that says any of them was done. `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/COPILOT_APP_PARITY_2026-09-28.md:90 @"So Verevon needs **Microsoft publisher verification**"`
- **No privacy-policy or terms-of-service page** among the public site's routes, and the sign-in page shows the sentence about accepting them without a link. A name search found no other page; the owner supplies the live URLs. `apps/Frontend Plane/verevonv3/src/features/auth/components/sections/AuthFormPanel.tsx:282 @"auth-terms"`
- **No startup warning** for a missing registration fact. The provider check only requires an id, and a secret where the mode needs one. `apps/Ingestion Plane/integration-corev2/internal/config/config.go:621 @"func (c Config) ValidateProvider"`
- **Two stale guides.** `apps/Control Plane/docs/SSO_SETUP_GUIDE.md` recommends a single-tenant registration and application permissions (`Directory.Read.All`, `Organization.Read.All`) that the sign-in code does not use. The root `INTEGRATION_CORE_IMPLEMENTATION.md` (2026-05-30) describes a single-tenant registration with a `localhost:3000/onboarding/connect/callback` redirect and a different permission list. Do not use either as a checklist. `apps/Control Plane/docs/SSO_SETUP_GUIDE.md:66 @"Account types: Single tenant (recommended) or Multitenant"`<br>`apps/Control Plane/docs/SSO_SETUP_GUIDE.md:100 @"Directory.Read.All (Application)"`<br>`INTEGRATION_CORE_IMPLEMENTATION.md:155 @"Accounts in this organizational directory only"`<br>`INTEGRATION_CORE_IMPLEMENTATION.md:156 @"onboarding/connect/callback"`

## 2. Microsoft Entra checklist (owner)

Do it once per registration (sign-in and connector). A step is done when its evidence is recorded in section 5.

| # | Step | Where | Evidence to record |
|---|---|---|---|
| M1 | **Confirm the registrations live in a Microsoft Entra work or school tenant owned by the company**, not under a personal Microsoft account. A registration made with a personal account cannot be publisher verified. `[vendor doc]` | Entra admin centre, App registrations | Tenant id; the two application (client) ids; whether they are one application or two. |
| M2 | **Supported account types: any organisational directory (multitenant).** The code defaults both audiences to `common`, which also admits personal Microsoft accounts. Decide whether personal accounts may sign in or connect; `organizations` admits business and school accounts only. | Authentication, supported account types | The chosen audience, and the value of `MICROSOFT_TENANT_ID` and `AZURE_TENANT_ID` in the service environment. |
| M3 | **Platform type and redirect URIs.** Sign-in registration: platform Web, with a client secret or a certificate. Connector registration: the platform type must match `MICROSOFT_CLIENT_AUTH_MODE`. In `public` mode the code sends an `Origin` header and no secret, which Microsoft documents for redirect URIs registered under the **Single-page application** platform; for such URIs Microsoft states that refresh tokens expire after 24 hours and the user must re-authenticate. A server-side connector that needs to keep a connection alive between logins is the case the **Web** platform with `confidential` mode (secret or certificate) is for, and Microsoft states no fixed lifetime for web-app refresh tokens. `[vendor doc]` This is an owner decision; section 3 notes the consequence for the sign-in hand-off. | Authentication, platform configurations | The platform type of each redirect URI as shown in the portal; the value of `MICROSOFT_CLIENT_AUTH_MODE`. |
| M4 | **Redirect URIs, exact and HTTPS.** Sign-in: `<BETTER_AUTH_URL>/api/auth/callback/microsoft`. Connector: `<INTEGRATION_PUBLIC_BASE_URL>/oauth/callback/microsoft`. Connector admin consent, **only once task G11 ships**: `<INTEGRATION_PUBLIC_BASE_URL>/oauth/callback/microsoft-admin` (no such route exists today, so do not describe it as live). Keep `http://localhost` entries off the production registration. The admin-consent endpoint requires its redirect URI to match a registered one. `[vendor doc]` | Authentication | The list of registered URIs per registration. The production values of the two base URLs. |
| M5 | **API permissions: delegated only, matching the bundles of section 1.2** for the connector, and the sign-in list of section 1.1 for the sign-in registration (subject to D3b). No application permissions. Mark the ones Microsoft says need an admin (section 3). | API permissions | The permission list as shown, each marked "admin consent required: yes or no" with the page it came from. |
| M6 | **Publisher domain and branding.** Set a publisher domain (it cannot be `*.onmicrosoft.com` for verification); verify the custom domain in the tenant; set name, logo, home page, support URL, privacy statement URL and terms of service URL. `[vendor doc]` | Branding and properties | The domain; the four URLs; a screenshot of the branding page. |
| M7 | **Microsoft AI Cloud Partner Program account (formerly MPN).** The Partner One ID must belong to the verified partner global account of the company; the Entra tenant must be associated with it; the person doing it needs multifactor sign-in, Application Administrator or Cloud Application Administrator in Entra, and the Partner Center admin role. `[vendor doc]` | Partner Center; then Branding and properties, "Add Partner ID to verify publisher" | The Partner One ID (an identifier, not a secret); the date; who did it. |
| M8 | **Check the badge.** Force a consent prompt with `prompt=consent` (test only, never in the product) and confirm the blue verified badge shows. `[vendor doc]` | A test sign-in | A screenshot of the consent prompt with the publisher name and badge. |
| M9 | **Credentials.** Create the client secret or certificate the chosen mode needs, record only the expiry date and the rotation owner, and put the value in the service environment file. | Certificates and secrets | Expiry date and owner for each credential. Never the value. |
| M10 | **Test on a business tenant** with the default consent setting: one non-admin member and one admin. Connect Microsoft 365 as each; sign in as each. Note what each sees. | A business test tenant | What the non-admin sees at sign-in and at connect (screenshots), and what the admin sees. |

## 3. D3b: the sign-in scopes (owner decision)

**The question.** The sign-in registration asks every person who signs in for twelve scopes, among them `Files.Read.All`, `Sites.Read.All`, `ChannelMessage.Read.All`, `Chat.Read` and `Mail.Read` (section 1.1). The plan asks whether a non-admin business user is blocked at sign-in itself.

**What Microsoft's documents say** (read 2026-10-01, `[vendor doc]`; confirm on a real tenant before relying on it):

- `ChannelMessage.Read.All` is a delegated permission that needs admin consent, per the Graph permissions reference's delegated table.
- With Microsoft's recommended default consent setting ("Let Microsoft manage your consent settings", the default for a new tenant), end users can consent to delegated permissions except a named list. That list includes `Files.Read.All`, `Sites.Read.All`, `Mail.Read` and `Chat.Read`. `Team.ReadBasic.All`, `Channel.ReadBasic.All`, `User.Read` and the sign-in scopes are not on that list.
- Where risk-based step-up consent is enabled, users cannot consent to most newly registered multitenant apps that are not publisher verified once they ask for more than basic sign-in and profile.
- The repository's earlier audit (2026-09-28) read the consent-policy page and concluded that under the default setting users cannot self-consent to `Calendars.ReadWrite`, `Files.ReadWrite.All`, `Sites.ReadWrite.All` or `Sites.Read.All`. The audit disagrees with itself on whether delegated `Sites.ReadWrite.All` needs admin consent in the per-permission table (its prose says only the application permission does, its fact-check row W4 says the delegated one does), so check that row of the permissions reference too. `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/COPILOT_APP_PARITY_2026-09-28.md:87 @"Under it, users cannot self-consent"`<br>`apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/COPILOT_APP_PARITY_2026-09-28.md:207 @"Delegated Sites.ReadWrite.All DOES require admin consent"`

**Reading, not verified on a tenant.** Five of the twelve sign-in scopes (`Files.Read.All`, `Sites.Read.All`, `ChannelMessage.Read.All`, `Chat.Read`, `Mail.Read`) are ones a non-admin cannot consent to under the default setting, and the plan named only three. A non-admin in such a tenant would meet an admin-approval wall at sign-in, before reaching the product, until an admin consents. The connector's default connect (section 1.2) has the same problem for the same permissions.

**Options and what each costs.**

| Option | What changes | Cost |
|---|---|---|
| A. Keep the wide sign-in scopes | Every business tenant needs an admin to approve the sign-in registration before its members can sign in. Needs a tenant admin-consent path for the sign-in registration, which task G11 does not build (it targets the connector). | Sign-in is blocked for non-admins until an admin acts, and a new tenant's first user may be the blocked one. |
| B. Narrow the sign-in registration to `openid profile email offline_access User.Read` | Non-admins can sign in. Connections to SharePoint, Teams and mail go through the connector's own connect flow, per bundle, with the admin-consent path of task G11. | The sign-in hand-off stops widening the organisation's connection on every login, so the connection no longer refreshes itself from sign-in (`microsoft-signin-handoff.ts`); the connector's own refresh token carries it, and its lifetime depends on the platform type chosen in M3. |
| C. Two sign-in entries | A light sign-in (profile scopes) by default and an "enterprise" sign-in with the wide scopes after the admin has consented. | Two registrations or two scope sets to keep correct, and a choice the user has to understand. |

Engineering reading, not a decision: B is the least-privilege shape and the one a verified publisher can defend on the consent screen; its cost is the hand-off behaviour above and is recoverable.

**Decision record** (owner fills in): chosen option; date; whether the hand-off comment and `auth.ts` scope list are to change (a code task, not part of this document); who tells the design partner.

## 4. Google checklist (owner)

| # | Step | Where | Evidence to record |
|---|---|---|---|
| G1 | **Decide one OAuth client or two** (sign-in and connector). Keeping them separate keeps the sign-in consent screen limited to profile scopes. | Google Cloud console, APIs and services, Credentials | The client ids (identifiers, not secrets) and which service uses which. |
| G2 | **Consent screen: user type External**, publishing status **In production**. An Internal screen serves only the company's own Workspace users, and a screen in Testing status is for test users: Google issues refresh tokens that expire in 7 days to such apps when they ask for more than basic profile scopes. `[vendor doc]` | OAuth consent screen | User type; publishing status; screenshot. |
| G3 | **Branding.** App name, logo, support email, home page, privacy policy URL, terms of service URL, authorised domains, developer contact. Brand verification runs in the console. `[vendor doc]` | OAuth consent screen (branding) | The URLs; the brand verification result and date. |
| G4 | **Domain ownership** of every authorised domain, through Google Search Console. `[vendor doc]` | Search Console | The verified domains. |
| G5 | **Declare exactly the scopes of section 1.3** on the consent screen (data access), and read the tier the console shows next to each. Prefer `drive.file` to `drive` for writes. | Data access | A screenshot of the scope list with the console's tier labels. |
| G6 | **Sensitive-scope verification** if any sensitive scope is requested: a justification for each scope, a demonstration video showing the consent flow, the app name and the client id, and a privacy policy that discloses how Google user data is accessed, used, stored and shared. Google gives about three to five business days for sensitive scopes. `[vendor doc]` | Verification centre | Submission date, reviewer replies, the approval date. |
| G7 | **Restricted-scope security assessment.** The connector asks for three restricted scopes (`drive.metadata.readonly`, `drive.readonly`, `gmail.readonly`), and it reads and stores the data on its own servers. Google requires an annual security assessment (CASA) for apps that reach restricted data from or through a third-party server, and re-verification at least every 12 months. `[vendor doc]` This has an external lead time that no engineering work shortens. | Verification centre | The assessor, the dates and the result. Or the owner's decision to remove restricted scopes instead (for example by moving reads to `drive.file` and picker-chosen files). |
| G8 | **Enable the APIs** the plan needs (Calendar, Sheets) in the project. | APIs and services | A screenshot of the enabled list. |
| G9 | **Redirect URIs, exact and HTTPS.** Sign-in: `<BETTER_AUTH_URL>/api/auth/callback/google`. Connector: `<INTEGRATION_PUBLIC_BASE_URL>/oauth/callback/google`. | Credentials, OAuth client | The registered list per client. |
| G10 | **While unverified**, the console shows an unverified-app warning and caps new users (Google states 100 in total after the screen appears). Do not onboard a customer on an unverified client. `[vendor doc]` | A test sign-in | A screenshot of what a new user sees. |

## 5. Evidence to record when each step is done

One line per step, in the owner's own tracker or appended below. Record identifiers, dates, initials and screenshots. **Never record a secret, a token, a client secret, a certificate or a password**, and never paste one into chat.

| Step | Done on (date) | By (initials) | Evidence kept (file name or link, no secrets) |
|---|---|---|---|
| M1 |  |  |  |
| M2 |  |  |  |
| M3 |  |  |  |
| M4 |  |  |  |
| M5 |  |  |  |
| M6 |  |  |  |
| M7 |  |  |  |
| M8 |  |  |  |
| M9 |  |  |  |
| M10 |  |  |  |
| D3b decision |  |  |  |
| G1 |  |  |  |
| G2 |  |  |  |
| G3 |  |  |  |
| G4 |  |  |  |
| G5 |  |  |  |
| G6 |  |  |  |
| G7 |  |  |  |
| G8 |  |  |  |
| G9 |  |  |  |
| G10 |  |  |  |

## 6. What this blocks, and what it unblocks

- **Task G11** (admin-consent path and ask-your-admin card) cannot be proven live until M4, M7 and M10 are done, because without a verified publisher the consent screen says "unverified". The v2 admin-consent endpoint takes an explicit scope list as well as `/.default`, so G11 can request one bundle at a time; confirm on a real tenant which URL behaves as needed. `[vendor doc]`
- **The write bundles** of the plan (calendar write, Excel, Drive write) should not be added to a registration before their bundle ships (section 1.2).
- **A business customer** can reliably connect only after M3 to M10 and G1 to G7 are done for the scopes that customer's bundle needs.

## 7. What this document does not claim

- Nothing about the owner's tenants, projects, Partner Center account, publisher status, consent-screen status, domains or existing credentials: none of it is in the repository, and none was read.
- Nothing about what any vendor will decide. The vendor statements above are pointers read from public pages on 2026-10-01 and must be confirmed in the vendor's console.
- Nothing about the sign-in registration's real redirect URI or platform type; confirm both in the portal.
