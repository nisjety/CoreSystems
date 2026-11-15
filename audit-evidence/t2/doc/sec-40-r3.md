<a id="F-t2-049"></a>
### F-t2-049 · URL and domain reputation gate before fetch and navigation, then layered screening (reviewer model, Quarry-side injection labels)
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane + Model Plane (reviewer) · Wave: W3 · Confidence: High*

**What and why.** Automate and extend the host blocklist Quarry already loads so every fetch, redirect, click-driven navigation and subresource is checked, with a per-org mode and a decision source in egress receipts. Autonomous browsing can reach phishing or malware pages, and enterprise customers expect their own web-gateway policy to apply to agent traffic. Reviewer-model screening and Quarry-side prompt-injection labels are optional follow-ups.

**Reference practice.** Firecrawl is the only reference with a reputation gate: 1 of 4 references [SRC]. Its checkUrl runs local allow and deny lists, optional customer secure-web-gateway categories and a hash-prefix or batched reputation provider before the first engine and on every redirect and discovered link, with an explicit fail-open or fail-closed policy [SRC]. Codex adds a tool-less reviewer with a per-origin cache and short expiry [SRC].

**Verevon today and approach.** An operator-run URLhaus/PhishTank sync (apps/Ingestion Plane/scripts/sync-security-feeds.py:1-12) writes a snapshot that quarry-edge loads once at boot (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs:345-372); SecurityEngine.preflight and validate_navigation consult it with exact-host matching (quarry-security/src/preflight.rs:22-60). Missing: scheduled sync, hot reload, subdomain matching, per-org modes, and enforcement on redirects, clicks and subresources, which use only the SSRF heuristic (quarry-browser/src/navigation.rs:1-35). Approach: scheduled sync with RwLock swap, suffix matching, org mode via the policy floor ([F-t2-022](#F-t2-022)), and a reputation callback in BrowserEgressPolicy so receipts carry a decision source. Add a hash-prefix provider only if feed coverage proves insufficient; never persist verdicts or send full URLs off-platform for Zero-Data-Retention (ZDR) orgs.

**Dependencies and risks.** Layers 2 and 3 need Quarry-to-Model grants and an ADR and must never be the sole gate for irreversible tiers; false positives need an allow-list and a clear block message.

<a id="F-t2-050"></a>
### F-t2-050 · Fail-closed PII redaction stage for scraped and crawled content
*Verevon today: Planned · Effort: L · Plane: Ingestion Plane (stage) with Model Plane (PII model) and Control Plane (policy) · Wave: W3 · Confidence: High*

**What and why.** Add an optional stage that removes personally identifiable information (PII) from scraped and crawled content before anything is persisted, and that fails closed. Orgs could then ingest web content without storing personal data, which complements ZDR. Today only model input is filtered; what Quarry stores is not.

**Reference practice.** Firecrawl is the only reference with this stage: 1 of 4 references [SRC]. After markdown derivation it sends markdown plus mode, entity list and replace style to a redaction service with several accuracy tiers; if the call fails or the size limit is exceeded it returns empty markdown with a warning [SRC].

**Verevon today and approach.** PageRunner builds markdown and metadata (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/pipeline.rs:505-516) and persists raw html, markdown and meta.json (pipeline.rs:604-660) without redaction. The existing redact_sensitive whitespace-tokenizes and would flatten markdown (quarry-runtime/src/agent_memory.rs:49-67); the Model Plane PII route takes no entity or replace-style parameters (apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:1947-1957) and quarry-edge holds no model-gateway grant. SOVEREIGN_SCRAPING_RELIABILITY.md:97 and :218 already plan a PII Redactor stage. Approach: a deterministic redactor in quarry-transform (regex plus checksum validators for email, phone, IBAN, card Luhn and the Norwegian fodselsnummer) behind a per-request option and an org default from capability-core safety_policies, which owns PII policy rather than the Control Plane (model-gateway/src/moderation.rs:3-5). Fail closed at the persistence boundary: no html, markdown, meta, index or ingest write on error. Add the Model Plane PII model as a second tier after the grant lands.

**Dependencies and risks.** Model-gateway grant (shared with [F-t2-019](#F-t2-019)); the PII route is third-party processing under the PrivacyPolicy gate; over-redaction and non-English accuracy need tests.

<a id="F-t2-051"></a>
### F-t2-051 · Procedure store and run-as-procedure, with postconditions so real runs can compile
*Verevon today: Partial · Effort: XL · Plane: Ingestion Plane · Wave: W4 · Confidence: High*

**What and why.** Let a successful agent browsing session become a saved, versioned procedure that replays through the normal step path with grants, approvals and budgets re-applied. This is the step from "the agent browses" to "the product automates", and a replay is cheaper than re-planning. Quarry already has the hard half: compile, replay-compare, quality and impact checks.

**Reference practice.** Firecrawl is the best source: 1 of 4 references [SRC]. Its agent route returns a skill, workflow and schema derived from a finished run, but the generator is closed upstream so only the route shape is known [SRC]. Apify adds immutable versioned builds with tags and records the resolved build on each run [DOC].

**Verevon today and approach.** compile_procedure, compare_replay, assess_quality and analyze_impact exist behind four stateless routes (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/browser_procedure.rs:61-200), but nothing stores the result, version is hard-coded to 1 and no execute route exists. compile_procedure rejects any receipt not marked Verified (browser_procedure.rs:85-96) while click, type, press, scroll, select and evaluate are Unknown (quarry-runtime/src/observation.rs:217-262), so a form-driving run cannot compile; ref-based actions embed snapshot_id and ref_id (quarry-core/src/contracts.rs:650-654) and cannot replay verbatim. Approach: add a step-level expect assertion Quarry evaluates to upgrade outcomes, canonicalise stored actions to SemanticLocator or ElementFingerprint, and fail closed on ambiguity. Put the store and runner in the Model Plane (capability-core or session-core), proposing each stored step to Quarry's step endpoint, because Quarry's own docs place rollout state outside Quarry (quarry-edge/src/agent_routes.rs:2462-2466). Stage: postconditions (M), canonicalisation (M), store plus runner (L).

**Dependencies and risks.** Needs [F-t2-001](#F-t2-001), [F-t2-008](#F-t2-008), [F-t2-010](#F-t2-010); never reuse a stored sensitive approval; ZDR runs write no receipts and cannot be saved.

<a id="F-t2-052"></a>
### F-t2-052 · Per-run result sets with a dataset table view and server-side export
*Verevon today: Absent · Effort: M · Plane: Ingestion Plane · Wave: W4 · Confidence: High*

**What and why.** Give every crawl and batch run an inspectable result set with a table view in the Ingestions page and streamed CSV and JSONL export. Users extracting prices, listings or procurement notices need a table they can filter and export without forcing everything into the Data Plane; ingest stays an explicit, selective decision.

**Reference practice.** Apify is the only reference with per-run storages: 1 of 4 references [DOC]. Run creation allocates a dataset, a key-value store and a request queue; the dataset is append-only and exports server-side in csv, jsonl, xlsx, xml or rss with field projection, and unnamed storages expire after about seven days [DOC].

**Verevon today and approach.** The edge route table has artifact list and get, jobs, events, sources, snapshots and team aggregates only (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs:130-293); ArtifactSummary has no run_id although keys are laid out org/run/page_hash/kind/id (quarry-core/src/resources.rs:83-97). The Ingestions page shows no result data (apps/Frontend Plane/verevonv3/src/features/ingestions/components/VerevonIngestionsPage.tsx:67-114), and /v1/extract is synchronous and persists nothing. Approach, lighter than a new JSONL store: (1) add run_id to ArtifactSummary and a run filter on /v1/artifacts (S); (2) project per-page rows from the page_extracted events already stored per job (routes.rs:776-830) plus artifact refs as GET /v1/runs/{id}/items with streamed csv and jsonl (M); (3) a table view in run detail with an ingest action. Tie a durable structured dataset to durable extraction jobs, which do not exist yet.

**Dependencies and risks.** Crawl and batch are refused under ZDR (routes.rs:555-575); artifact reads are org-bound, not per actor; keep it a working set with retention, not a second Data Plane.

<a id="F-t2-053"></a>
### F-t2-053 · Saved job definitions: expose presets, then first-party Quarry packs with typed schemas
*Verevon today: Partial · Effort: L · Plane: Ingestion Plane (with Frontend Plane for forms) · Wave: W4 · Confidence: High*

**What and why.** Give users and the Model Plane a menu of saved, typed job definitions instead of free-form crawls, with Quarry validating input and keeping execution authority. Phase 1 is mostly making dormant code work: stored scrape, crawl and batch schedules never fire, and presets are decorative at execution time.

**Reference practice.** Apify is the best source and Codex a second: 2 of 4 references [DOC, SRC]. Apify declares one manifest per unit with input and output JSON Schema whose UI hints render the form and validate API input identically; tasks are saved input plus run options that can be run, scheduled and shared [DOC]. Codex gates bundled plugins with an availability check before install [SRC].

**Verevon today and approach.** GET /v1/presets exists only on quarry-control (apps/Ingestion Plane/Quarry-v2/services/quarry-control/cmd/control/main.go:174), unproxied by edge; every PageRunner is built with RunPolicy::default() (quarry-edge/src/routes.rs:461) and the orchestrator never reads its Policy field. Schedules accept cron for any kind, but control maps only change_monitor to a workflow (quarry-control/internal/resources/schedule_wire.go:37-52) although Scrape, Crawl and Batch workflows are registered (quarry-orchestrator/cmd/orchestrator/main.go:94-118). A durable Sources registry already overlaps "save as preset" (quarry-edge/src/resource_routes.rs:410-426). Approach: (a) extend workflowFor so non-monitor schedules fire with a per-fire run_id; (b) define a saved definition as a Source plus request-level options, or drop the Go RunPolicy presets; do not expose /v1/presets until policy is enforced, since the fast preset ignores robots (presets.go:69); (c) rename the overloaded term. Demote the QuarryPack manifest and schema form until a second consumer exists.

**Dependencies and risks.** Saved crawl and batch jobs are refused under ZDR; Space-shared definitions must reference only sources the viewer already sees; keep packs declarative and first-party.

<a id="F-t2-055"></a>
### F-t2-055 · Crawl planning: validate/preview endpoint, Model Plane crawl-plan tool and a stronger /v1/map
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane (validate, map) with Model Plane (planner tool) and Frontend Plane (review UI) · Wave: W4 · Confidence: High*

**What and why.** Let a user scope a crawl in one sentence: a Model Plane tool maps the site, drafts crawl options from the goal, and the user approves the proposed scope before the gateway starts the crawl under the user token. This fits "Model proposes, Quarry validates and executes" and improves discovery on docs sites with weak sitemaps.

**Reference practice.** Firecrawl is the only reference with natural-language crawl planning: 1 of 4 references; on site mapping Verevon is already at parity [SRC]. Firecrawl maps up to 50 URLs to ground the model, overlays only fields the caller did not set, re-runs the normal validator and returns a preview or starts the crawl; the ZDR path keeps the model step outside traces [SRC].

**Verevon today and approach.** /v1/map is shipped end to end with robots allow-filtering and sitemap.xml seeding (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/map_routes.rs:132-300) and CrawlPagePicker lets users curate pages. There is no /v1/crawl/validate, and CrawlRequest carries only url, max_pages, max_depth, ingest and zdr (quarry-edge/src/routes.rs:533-554), so there are no scope options to validate until [F-t2-016](#F-t2-016). Map ignores the Sitemap: lines that robots.rs:80-82 already parses, ranks lexically (map_routes.rs:266-270) and is closed to service principals (quarry-edge/src/auth.rs:504-535); execution-core web_tools declares only search:read and extract:read (apps/Model Plane/rust/services/execution-core/src/web_tools.rs:39-40). Approach: map:read service scope and robots Sitemap seeding first (both S), then the planner tool (M); cut validate to a thin map-based preview; extend CrawlPagePicker to show the proposed scope instead of a new review UI; use the granted Data Plane web-rerank row only when its token is available, degrading to lexical order.

**Dependencies and risks.** Crawl is refused under ZDR, so a ZDR plan can be proposed but not run; always show drafted scope before running; a site: search pass spends paid-provider budget.

<a id="F-t2-056"></a>
### F-t2-056 · Provider APIs and export rewrites before rendering (Google Docs, GitHub, Notion, transcripts), after verifying current coverage
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane (Quarry planner + integration-corev2) · Wave: W4 · Confidence: Medium*

**What and why.** When a user or agent pastes a Google, Microsoft or GitHub document link, return the document through the user's connected provider account instead of a rendered login wall or a terms-of-service-risky scrape. integration-corev2 already owns the OAuth tokens and read actions, so the work is routing and agent-visible content, not new connectors.

**Reference practice.** Firecrawl is the best source: 1 of 4 references [SRC]. Its URL normaliser rewrites Google document links to export URLs, and an engine resolver calls a provider API when the host matches a connected account, returning a normal scrape result [SRC]. Codex exports signed-in documents through the page session with strict URL validation [SRC, DOC]; Verevon should use OAuth provider actions instead, for credential isolation [INF].

**Verevon today and approach.** integration-corev2 ships Google drive.file.content with text or Office export (apps/Ingestion Plane/integration-corev2/internal/actions/drive_read.go:30-60), Microsoft drive.file.content, GitHub contents.get and readme.get (internal/actions/service.go:219, 348, 366) and list-only Notion pages (service.go:463-466). Model Plane agents reach integration-corev2, but the agent catalog omits every content op (apps/Model Plane/rust/services/execution-core/src/integration_tools.rs:98-141), so such calls need approval as unknown-means-write (execution-core/src/permission/mod.rs:195-218) and results are cut at 2000 characters (integration_tools.rs:817-825). Quarry has no rewrite table, provider resolver, transcript format or /v1/parse. Approach: (1) a read-only document tool at the Model Plane or gateway that maps URL to provider and file id and decodes content to bounded text (S to M); (2) a small public-URL rewrite table in Quarry routed through the file router ([F-t2-009](#F-t2-009)) (S); (3) demote the caption transcript format; no Quarry /v1/parse without a quality comparison.

**Dependencies and risks.** Reads must use the requesting user's own connection; provider ACL and ZDR must travel with ingested content; Google and Microsoft content ops are not yet verified against live providers; Notion content needs a new action.

<a id="F-t2-058"></a>
### F-t2-058 · Scrape request context and egress tiers: locale/geo/mobile emulation, country egress with policy-gated auto-escalation, profile pinning and cache partitioning
*Verevon today: Partial · Effort: XL · Plane: Ingestion Plane + Control Plane (tier cap, metering) · Wave: W4 · Confidence: High*

**What and why.** Let a scrape carry request context (locale, timezone, geolocation, device, an authenticated profile, a country egress tier) so a Norwegian-first product can fetch localized content, and so personalised pages never leak across tenants through a shared cache. Policy, never the Model, picks the proxy tier.

**Reference practice.** Firecrawl is the best source: 1 of 4 references for profile routing and location, 2 of 4 (Firecrawl, Apify) for tiers and geo proxies [SRC, DOC]. Firecrawl restricts profile requests to profile-capable browser engines, treats headers, actions or profile as personalised with no cache use, sets Accept-Language and timezone from a validated country, and retries once on a stronger pool after a block signal [SRC]. Apify adds locale-constrained fingerprints and a scored SessionPool [DOC].

**Verevon today and approach.** ScrapeRequest has no profile_id, location, device or proxy field (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs:346-375); profile_required arrives only through the client-supplied signals object (quarry-runtime/src/driver_plan.rs:36-48). The local driver sets no user-agent, locale, timezone or geolocation override (quarry-browser/src/chromiumoxide.rs:1851-1860) and relaunches the shared Chromium when the viewport changes (chromiumoxide.rs:194-270). Egress is richer than assumed, a processor-gated pool with rotation on 401/403/407/429/503 (quarry-runtime/src/egress_broker.rs:102-163), but has no tier, country or request-level field, and the Chromium path has no upstream proxy chaining (quarry-runtime/src/browser_egress_proxy.rs:1-8). The cache is content-addressed, not option-keyed (quarry-edge/src/cache.rs:1-36), so the cache-key premise is wrong. Approach, in slices: (a) per-page emulation with session-scoped contexts, needs [F-t2-007](#F-t2-007) (M); (b) profile pinning for scrape with a hard no-cache rule and an org prefix on the cache key (M); (c) egress tiers and country behind the processor gate, browser chaining designed first (L); (d) defer SessionPool.

**Dependencies and risks.** [F-t2-020](#F-t2-020), [F-t2-022](#F-t2-022), [F-t2-027](#F-t2-027) to narrow block statuses before escalation; proxies are processors that resolve DNS server-side, so the SSRF boundary needs an explicit decision; residential tiers stay off by default.

<a id="F-t2-059"></a>
### F-t2-059 · Register governed remote browser backends (Kernel, Browserless, Browserbase) behind an egress-containment proof
*Verevon today: Partial · Effort: XL · Plane: Ingestion Plane (quarry-browser, quarry-edge) · Wave: W4 · Confidence: High*

**What and why.** Give the in-app browser and agent runs a scale-out path beyond the single local Chromium by registering remote backends with declared capability sets, so the Model states requirements and Quarry selects or rejects a backend without the Model ever naming a provider. Capability flags and the isolated-egress gate already exist, so governance stays intact.

**Reference practice.** Codex is the only reference with a multi-backend list: 1 of 4 references [SRC]. Backends are registered from config at startup, the session API lists them with type, family, profile and metadata, and the planner chooses by required capabilities such as user login or isolated egress [SRC].

**Verevon today and approach.** Capability flags and gates exist (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/lib.rs:265-286; quarry-edge/src/agent_routes.rs:1125-1217), but the agent path is hard-wired to one ChromiumoxideDriver (quarry-edge/src/main.rs:1283-1288) and the scrape path has a single Browser slot that Browserbase takes first (main.rs:322-339). Kernel and Browserless are one-shot REST shims built only in tests (quarry-browser/src/kernel.rs:1-40, browserless.rs:1-30) that cannot serve ref actions, AX snapshots or live frames, so the premise that they are ready to register is wrong. Approach in three parts: (1) scrape path only, key the registry beyond DriverKind::Browser and register Kernel or Browserless behind config with the managed_provider processor guard (S to M); (2) for agent sessions, a CDP-attached remote driver reusing ChromiumoxideDriver over Browser::connect, a backend registry in AppState and a real containment proof (XL); (3) GET /v1/agent/backends with selection by the driver_requirements field already accepted on start (S).

**Dependencies and risks.** The pinned egress proxy is loopback-only (lib.rs:331-360), so routing provider traffic through it contradicts that invariant; remote providers are processors refused under ZDR and need DPA review; per-provider redirect, iframe and subresource SSRF tests are a documented gate.

<a id="F-t2-060"></a>
### F-t2-060 · Isolate each browser session or tenant in its own container or VM instead of a shared Chromium
*Verevon today: Absent · Effort: XL · Plane: Ingestion Plane · Wave: W4 · Confidence: High*

**What and why.** Run each org's (later each session's) browser in its own container or microVM with its own Chromium, tmpfs profile, cgroup limits and egress only through the egress broker. A browser that users and agents drive on arbitrary sites is a tenant boundary; a renderer exploit in a shared Chromium crosses tenants. Two cheap hardening fixes should ship first.

**Reference practice.** Apify is the only reference, and documentation only: 1 of 4 references [DOC]. It runs each Actor in an isolated per-run container under SOC 2 Type II without disclosing the runtime technology [DOC].

**Verevon today and approach.** Isolation is one Chromium per driver (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:194-262) with a per-session CDP context inside it (chromiumoxide.rs:396-420). ensure_browser relaunches that shared Chromium, clearing every session's pages, when a lease asks for a different viewport (chromiumoxide.rs:194-223), so concurrent tenants can kill each other's runs. The compose default disables the Chromium sandbox when QUARRY_BROWSER_NO_SANDBOX is unset, contradicting its own comment (apps/Ingestion Plane/Quarry-v2/deploy/compose/docker-compose.yml:97-99); production manifests were not checked. sandbox-manager is only a lease registry that starts nothing (apps/Model Plane/go/services/sandbox-manager/internal/server/server.go:84-97) and is the wrong plane, so that option is dropped. Approach: step 1 (S to M, quick win), apply viewport per page or context through CDP emulation and flip the compose default; step 2 (L), a CDP-connect mode on ChromiumoxideDriver and a per-org pool of Chromium worker containers, each with its own pinned-proxy sidecar, owned by the Ingestion Plane; step 3 (XL, optional), microVM per session. The Kernel and Browserless shims are not a shortcut.

**Dependencies and risks.** The loopback-only proxy type needs a sidecar design or a reviewed relaxation; profile export stays blocked under ZDR; scrape-path sessions are keyed by host only (quarry-runtime/src/browser_driver.rs:139-148), so same-host scrapes from different orgs may share a session key [INF, not tested].

<a id="F-t2-062"></a>
### F-t2-062 · Budgeted web research job contract with run-context cost attribution
*Verevon today: Partial · Effort: M · Plane: Model Plane (run contract) with Ingestion Plane (run-context attribution) · Wave: W4 · Confidence: Medium*

**What and why.** Make web research an addressable run with a caller budget, partial results, a stop reason and one active run per thread, and let Quarry attach its cost events to the parent run so nothing is billed twice. One predictable contract serves other features better than stitching step-level routes, and per-run cost depends on Quarry knowing the parent run.

**Reference practice.** Firecrawl is the only reference with a research-job API: 1 of 4 references [SRC]. POST /agent takes prompt, optional urls, schema, a credit cap and effort; GET returns status, data, partial, schema validity, stop reason and credits used; a thread rejects a new turn with 409 while a run is active, and internal services pass a request id and a billing flag so tool calls attach to the parent run [SRC]. Apify caps Actor runs by maximum charge [DOC].

**Verevon today and approach.** A research pipeline already exists as a chat feature: model-gateway deep_research plans, searches, reads and synthesizes under the turn's session run with spend caps and a halt_code stop reason (apps/Model Plane/rust/services/model-gateway/src/deep_research.rs:58-140, 600-660), but it dies with the stream (deep_research.rs:1-24), takes no caller budget or output schema, and no thread-busy guard exists. execution-core web tools send only a bearer and x-quarry-org (apps/Model Plane/rust/services/execution-core/src/web_tools.rs:95-96), and Quarry usage events carry no run id (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/usage.rs:47-83); a USD ceiling exists only for browser runs. Approach: promote deep_research to an addressable run (status, partial results, stopReason from halt_code, 409 with the running run id), map the caller budget onto the existing caps plus a credit or USD ceiling enforced in the Model Plane, and add output-schema validation after [F-t2-019](#F-t2-019). Separately, send a signed requestId and parentRunId from web_tools and the gateway Quarry client, written into UsageEvent.metadata as advisory attribution, never authority.

**Dependencies and risks.** [F-t2-001](#F-t2-001); a durable job record must retain no content for ZDR callers; cost-core ingestion of Quarry cost was not traced; crawl starts remain approval-gated.
