# T2 / Category 10: Integration-plane overlap (comparison summary)

There are 31 reference records from apify (8), aside (4), codexbrowser (8) and firecrawl (11). All 31 are assigned to 19 clusters and none are excluded. Verevon has not shipped 14 of these clusters (1 planned, 5 partial, 3 absent, 5 unknown). Full detail is in `10-integration-plane-overlap.clusters.json`.

## Shared baseline (table-stakes)

No capability reaches table-stakes here. Two of the four sources are browsers (Codex, Aside) and two are web platforms (Firecrawl, Apify), and their integration surfaces barely overlap. The most widely shared clusters each cover 2 of 4 sources:

- **Outbound signed event webhooks** (apify, firecrawl; 005).
- **Exposing the platform to external agents via MCP or framework wrappers** (apify, firecrawl; 007).
- **Pushing data into knowledge stores or destinations** (apify, firecrawl; 011).
- **A credential-injecting connector proxy for agents** (apify, aside; 002).
- **Chat-app entry points** (aside, firecrawl; 004).

## Best of breed

| Capability | Best source (records) | Why |
|---|---|---|
| Connector secrets delivered into pages (001) | codexbrowser (132, 042) | Email OTP is filled only after user approval and under strict origin and field preconditions. The model never sees the code, and secrets reach the page through a broker socket. q5, SRC. |
| Credential-injecting connector proxy (002) | apify (102) | Server-side injection, access that ends with the run, and per-connector tool allow-lists. q5, DOC. |
| MCP client hardening (003) | aside (016) | HTTPS only and no redirects. Verevon's version is stronger. |
| Chat entry points (004) | firecrawl (108) | Signature-verified Slack commands and events, ack-then-async handling, encrypted bot tokens. |
| Outbound webhooks (005) | apify (101, 108) | Rich event vocabulary with payload and header templates. Firecrawl adds an SSRF refusal and delivery logs. |
| External-agent MCP plus auth (007) | firecrawl (105, 106) | OAuth tokens are introspected and bound to an audience. A delegated HMAC credential limits purpose and clock skew. |
| Tool-call audit logs (008) | firecrawl (107) | Team view with intent fields left out. Under ZDR nothing is stored (202). |
| Capability discovery by domain (009) | firecrawl (103, 099) | A search or scrape returns provider tools that match the page's domain, and one endpoint executes them. |
| Idempotent metered execution (010) | firecrawl (104) | Charge id is sha256(team, request id). Each request ends in one of five states, and unresolved outcomes are held for manual reconciliation. |
| Sandboxed MCP App UI (015) | codexbrowser (119) | Dedicated sandbox origin, a scheme allow-list, and an agent limited to locator actions. |

## Differentiators

- **Credential-injecting connector proxy (002).** Verevon already has this; see the strengths section.
- **Chat-app entry points (004).** Verevon status is unknown, and Channel Plane is still future work.

Unique items worth noting:

- **Connector-to-browser secret bridge (001, Codex).** This is the most relevant one for Verevon.
- **Domain-matched provider capability discovery (009, Firecrawl).**
- **Signed responses over a local pipe (016, Codex).** Not relevant to Verevon.

## Verevon's biggest gaps

1. **Connector-to-browser credential bridge (001)**
   - Tier: priority. Significance 4, effort L.
   - Integration-corev2 (vault, token broker) and Quarry (browser profiles) never meet at runtime.
   - The only plan is in the docs: a `fill_secret` action with a `vault://` secretRef, in reconciled plan §19.1.
   - Build it in Quarry as executor-side secret resolution through the IC token broker. It needs:
     - a BrowserBroker sensitive grant;
     - origin and field preconditions;
     - redaction of the filled value;
     - a user elicitation raised by Model Plane.
   - Blocked by: nothing issues BrowserBroker grants in production yet.
2. **Domain-matched capability discovery (009)**
   - Tier: recommended. Effort M.
   - Today the agent has a static curated CATALOG in execution-core (`integration_tools.rs:60-75`).
   - IC should serve a catalog with schemas and provider domains. The Model Plane would then offer API actions, or a "connect this provider" suggestion, when the browsed host matches.
   - This replaces brittle UI clicking with governed API calls.
3. **Tenant-scoped, SSRF-guarded outbound webhooks (005)**
   - Tier: recommended. Effort M.
   - The quarry-control dispatcher already has HMAC signing, 5 retries with backoff and a DLQ.
   - What it lacks:
     - an `org_id` on webhooks;
     - exposure through the edge;
     - a per-job webhook;
     - an SSRF guard. It uses a plain `http.Client` (`dispatcher.go:36-41`, `main.go:197`). Port the Model Plane `safe_mcp_http_client` rules before tenants can register URLs.
4. **MCP server or external-agent exposure of Quarry and connectors (007)**
   - Tier: recommended. Effort L.
   - Absent today; the backlog explicitly defers it as "Skip".
   - Needs audience-bound OAuth from Control Plane auth-core and short-lived, purpose-tagged delegation to Quarry and IC.
   - Writes must still require in-product approval.

The rest are nice-to-have:

- chat-app entry (004)
- no-code platform apps (006)
- tool-call audit view (008)
- Slack and opt-in alert destinations (012)
- first-party embedded sign-in (014)
- MCP App sandbox rendering (015)
- user-browser extension (017; XL, and it conflicts with server-side containment)
- tab mentions (018)

Two are skipped:

- signed local pipe (016)
- Apple Passwords bridge (019; desktop only)

## Verevon strengths (keep)

- **Idempotent provider writes (010).** Verevon is ahead of Firecrawl here:
  - write actions require an idempotency key;
  - a request SHA-256 detects conflicting retries;
  - a signed attestation identifies the acting user;
  - receipts move through executing, completed and unknown states;
  - everything runs inside an audit transaction whose outbox drains to Audit Core (`server.go:3093-3300`, `audit_outbox.go:46`).
- **MCP client hardening (003).** Ahead of Aside:
  - HTTPS only and `redirect::Policy::none`, with DNS pinned on every dial (`runtime_registries.rs:426-465`);
  - SSRF re-validation of the OAuth discovery, DCR and token hops;
  - DNS revalidation every 15 minutes, with quarantine;
  - a mandatory exact tool allowlist.
- **Connector proxy for agents (002).**
  - A first-party OAuth broker covers about 25 providers.
  - Tokens sit in an AES-GCM vault and are issued through an internal short-lived broker.
  - The model can call only named whitelisted operations; the raw proxy returns 410.
  - Writes are approval-gated.
- **Knowledge sinks (011).** Data reaches the Data Plane only through its ingest contract, with tenant isolation. Do not copy Firecrawl's cross-tenant shared index.
- **Provisioning (013).** SCIM is in place. Never adopt Firecrawl's secret-in-URL admin gate.

## Caveats

- All 8 Apify records are DOC. Clusters 006 and 018 rest only on DOC records. The DOC records in 002, 005, 007, 009 and 011 were not upgraded.
- Firecrawl's paid settlement (104, 099) is a design described in its README and covered only by mocked tests.
- Eleven clusters have no category-10 baseline id. Five are marked unknown (004, 006, 014, 016, 018). Four are partial, based on the baseline narratives or my spot checks (005, 008, 012, 015). Two are absent, per the v-browser-app narrative section 7 (017, 019).
  - I added code spot checks for 005, 008, 012 and 015. They are greps, not exhaustive audits.
  - Status for 005 comes from the v-quarry baseline narrative (§1) plus my reading of the dispatcher code.
- The missing SSRF guard on webhooks is an inference: the dispatcher receives a nil `http.Client`, and I found no guard on that path. The private-address test was not run.
- `DashboardSearchResults.tsx:184` combines `allow-scripts` and `allow-same-origin` in one iframe. That combination is worth a security review if the content can be same-origin. Not verified.
- The ranking assumes that BrowserBroker grant issuance gets fixed first. Without it, neither 001 nor any other agent-browser feature works in production.
