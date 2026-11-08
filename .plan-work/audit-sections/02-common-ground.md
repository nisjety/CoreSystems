## 2. What every reference shares

The four references split into two pairs: Firecrawl and Apify are acquisition platforms (scrape, crawl, search, jobs), Codex and Aside are browsers that a person and an agent share. 61 capabilities count as table-stakes: present in both members of a pair, or in 3 of the 4 references. A few rows fall below that bar, either because verification corrected the count or because the comparison classed a single-reference browser capability as table-stakes for a browser product; they are kept and the count shown is the corrected one. Verevon already ships 14 of the 61 at parity or better, mostly in the agent-control core (accessibility-tree observation, stale-target protection, locators, dialogs, approvals, risk tiers, evidence) and in platform plumbing (fetch engine, proxies, politeness, durable queue). The rest are mostly Partial, and the pattern is consistent: the code exists in Quarry v2 but is not wired into the live path, or is enforced in the Verevon gateway rather than in Quarry.

Prevalence is written as "n of N references". N is 4 for the acquisition categories (sections 2.1 to 2.7). The data counts the two browser categories (2.8 and 2.9) out of 3, because Apify, whose evidence is documentation only, has no browser shell to compare [INF]. Where verification corrected a count, the corrected statement is used and the original number is not repeated. "Verevon today" is the verified status of the recommendation that covers the capability; where one recommendation covers several capabilities, the status of the specific capability is given with the item status in brackets.

### 2.1 Acquisition API surface

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Synchronous single-URL scrape returning the document inline | 2 of 4 (Firecrawl, Apify) | Partial | [F-t2-005](#F-t2-005) |
| Site crawl with scoping controls | 2 of 4 (Firecrawl, Apify) | Stub | [F-t2-016](#F-t2-016) |
| Web search API with filters, categories and provider fallback | 3 of 4 (Firecrawl, Aside, Codex) | Shipped | none needed |
| MCP server exposure for external agents | 3 of 4 (Apify, Aside, Firecrawl) | Absent (item Partial, because an OpenAPI spec exists) | [F-t2-064](#F-t2-064) |
| CLI and installable agent skills | 3 of 4 (Apify, Firecrawl, Aside) | Absent (item Partial) | [F-t2-064](#F-t2-064) |

The shared baseline for an acquisition API is one scrape call that returns the requested formats inline, a crawl with include/exclude scope, and a search endpoint [SRC for Firecrawl, DOC for Apify]. Verevon's search is ahead of the references (multi-provider router with Zero-Data-Retention, ZDR, gating), but its scrape returns artifact references instead of inline text, so a ZDR scrape returns no text at all (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/pipeline.rs:597-601), and POST /v1/crawl accepts only url, max_pages, max_depth, ingest and zdr (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs:533-552). Model Context Protocol (MCP) exposure is the one capability three references ship and Verevon has deferred by decision (apps/Ingestion Plane/Quarry-v2/docs/OSS_PARITY_BACKLOG.md:30).

### 2.2 Output formats & content processing

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Main-content extraction with selector filters and selectable extractors | 2 of 4 (Firecrawl, Apify) | Partial | [F-t2-005](#F-t2-005) |
| Screenshot and page-print (PDF) output on scrape | 3 of 4 (Firecrawl, Codex, Aside) | Partial | [F-t2-023](#F-t2-023) |
| Office and e-book document conversion and file capture during crawl | 2 of 4 (Firecrawl, Apify) | Partial (DOCX extractor has no caller) | [F-t2-037](#F-t2-037) |

Both acquisition platforms let the caller choose how much of the page comes back (main content only, include or exclude selectors) and both convert office documents; Apify also falls back to the full HTML when a page is not a clear article [DOC]. Firecrawl runs a fixed transformer order and strips unrequested formats at the end [SRC]. Verevon runs one hard-coded Readability pass (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/pipeline.rs:506), hard-codes screenshot and pdf to None on the scrape path (pipeline.rs:702-703), and has no content-type router, so the DOCX and PDF extractors in quarry-transform are reached only from the answer fetcher (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/answer.rs:835-839).

### 2.3 Rendering & browser engines

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Multi-tier fetch engine core (HTTP, TLS-impersonating HTTP, headless browser behind one interface) | 2 of 4 (Apify, Firecrawl) | Shipped (Chromium only) | none needed |
| TLS/HTTP2 impersonation tier and browser-consistent headers | 2 of 4 (Apify, Firecrawl) | Stub | [F-t2-066](#F-t2-066) |
| Browser pool lifecycle and per-request context isolation | 1 of 4 (Apify Crawlee; the Firecrawl count was corrected: it runs one Chromium behind a page semaphore) | Partial | [F-t2-007](#F-t2-007) |
| Persistent sessions and browser profiles across runs | Recorded for Firecrawl and Apify; the count is an undercount, Codex also keeps persistent storage partitions [SRC] | Partial | [F-t2-020](#F-t2-020) |
| Declarative page actions on scrape (wait, click, write, press, scroll, screenshot, JS) | 2 of 4 (Apify, Firecrawl) | Partial | [F-t2-023](#F-t2-023) |

The engine baseline is a waterfall of cheap HTTP, TLS-impersonating HTTP and a real browser behind one interface, which Verevon has [SRC]. Where Verevon is behind is browser process management: one shared Chromium is relaunched, and every session's pages are dropped, whenever a lease asks for a different viewport (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:194-270). Profile persistence works end to end, but the snapshot captures only cookies for the current URL without expiry and no IndexedDB (chromiumoxide.rs:1771-1832), and the ActionRuntime that would run scrape-time actions has no production caller (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/browser_driver.rs:128-240).

### 2.4 Anti-bot, proxies & reliability

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Proxy pool plumbing and egress self-check | 2 of 4 (Apify, Firecrawl) | Shipped | none needed |
| Tiered, residential and geo-targeted proxies with escalation on block | 2 of 4 (Apify, Firecrawl) | Partial | [F-t2-058](#F-t2-058) |
| Retries, error hooks and resilient internal clients | 2 of 4 (Apify, Firecrawl) | Partial | [F-t2-047](#F-t2-047) |
| robots.txt compliance with governed override | 2 of 4 (Apify, Firecrawl) | Stub | [F-t2-016](#F-t2-016) |
| Politeness and throughput throttling | 2 of 4 (Apify, Firecrawl) | Shipped | none needed |

Both acquisition platforms respect robots.txt by default, throttle per host and retry with a per-request hook [SRC for Firecrawl, DOC for Apify]. Verevon's per-host politeness and proxy plumbing are at parity, and its egress containment (DNS pinning, per-hop redirect checks, deny-by-default network guard) is stronger than either reference. The robots gap is a compliance risk: robots rules are applied only in /v1/map, and RobotsCache itself has no production caller (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/map_routes.rs:87-109), and the Go crawl workflow has no robots check (apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/workflows/workflows.go:396-516). The retry helper is exported but only its own tests call it (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs:164).

### 2.5 Orchestration & scale

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Durable request queue / URL frontier with lease-based claims | 2 of 4 (Apify, Firecrawl) | Shipped | none needed |
| Crawl budgets and guardrails (page, depth, rate, cost caps, result TTL) | 2 of 4 (Apify, Firecrawl) | Partial | [F-t2-008](#F-t2-008) |
| Crawl scoping and link filtering with actionable denial reasons | 2 of 4 (Apify, Firecrawl) | Stub | [F-t2-016](#F-t2-016) |
| Sitemap, robots and index-assisted URL discovery | 2 of 4 (Apify, Firecrawl) | Partial | [F-t2-041](#F-t2-041) |
| Executable schedules with timezone, jitter, overlap skip and stale-run recovery | 2 of 4 (Apify, Firecrawl) | Partial | [F-t2-018](#F-t2-018) |

The baseline is a durable queue, a scoped frontier, sitemap seeding, budgets and schedules that run. Verevon's Postgres SKIP LOCKED queue with Temporal is at parity. Everything above it is scaffolding without a live caller: CrawlFrontier and CrawlDenialReason are never used by the Go workflow (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/crawl_frontier.rs, crawl_denial.rs), only max_steps is enforced on the agent path and crawls are bounded only by max_pages where 0 means unlimited (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:446-451; apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/workflows/workflows.go:456), and schedules map only change_monitor to a workflow, so scrape, crawl and batch schedules never fire (apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/resources/schedule_wire.go:39-51).

### 2.6 Platform & ecosystem

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Usage pricing function and metering (credits, failures free, feature surcharges) | 2 of 4 (Firecrawl, Apify) | Partial | [F-t2-046](#F-t2-046) |
| Plans, limits and plan-gated browser features | 3 of 4 (Apify, Firecrawl, Aside) | Partial | [F-t2-046](#F-t2-046) |

Every commercial reference has one pricing function and plan keys that gate web features [SRC for Firecrawl and Aside, DOC for Apify]. Verevon publishes five idempotent usage metrics to billing-core, but agent routes emit no usage event, so browser steps and minutes are unmetered, and the plan-key lists in org-core and billing-core contain no browser or Quarry keys (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/usage.rs:88-94; apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:700-761).

### 2.7 Observability & operations

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Request correlation ids and run origin attribution | 3 of 4 (Firecrawl, Codex, Apify) | Partial | [F-t2-014](#F-t2-014) |
| Run history with retention, TTL access and ZDR-scrubbed records | Firecrawl and Apify; the Aside count was corrected (its only evidenced retention is a 15-minute purge of ephemeral sessions) | Partial | [F-t2-003](#F-t2-003) |
| Least-privilege permissions for agents and run tokens | 3 of 4 (Apify, Aside, Codex) | Partial | [F-t2-036](#F-t2-036) |

The references agree on a correlation id per request, a run origin, a run history with a retention rule, and scoped, expiring run authority. Verevon has the stronger evidence model (immutable step receipts, checkpoints, proof bundles) but the operational layer around it is thin: quarry-edge mints its own request id per handler with no x-request-id intake (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:1739), ZDR runs leave no durable record, and BrowserBroker grants live in an in-memory map with no production issuer (apps/Model Plane/go/services/browser-broker/internal/grant/grant.go:62-71; apps/Model Plane/go/services/browser-broker/cmd/main.go:63).

### 2.8 In-app browser shell (user-facing)

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Tab strip operations and detached windows | 1 of 3 (Codex) | Partial | [F-t2-070](#F-t2-070) |
| Chat beside the browser: side panel, docked browser, floating composer | 2 of 3 (Aside, Codex) | Partial | [F-t2-012](#F-t2-012) |
| Navigation controls and free cross-site browsing | 1 of 3 recorded (Codex; free browsing is inferred from the absence of a human allowlist [INF]) | Partial | [F-t2-011](#F-t2-011) |
| Standard page utilities: find, zoom, print, mute, DevTools, media indicators | 2 of 3 (Codex, Aside) | Partial | [F-t2-069](#F-t2-069) |
| Browser profiles and persistent login state | 2 of 3 (Codex, Aside) | Shipped | none needed |
| Agent-control indicator, live view and human take-over | 3 of 3 (Aside, Codex, Firecrawl) | Shipped (takeover is enforced in the gateway, not in Quarry) | [F-t2-013](#F-t2-013) phase 1 |
| Screenshots and page capture (viewport, area, full page) | 2 of 3 (Aside, Codex) | Partial | [F-t2-069](#F-t2-069) |
| Browsing history and bookmarks with agent/user source | 2 of 3 (Codex, Aside) | Partial | [F-t2-068](#F-t2-068) |
| Privacy controls and clear browsing data | 2 of 3 (Aside, Codex) | Partial | [F-t2-068](#F-t2-068) |
| Browser and agent-permission settings hub | 2 of 3 (Codex, Aside) | Absent (item Partial) | [F-t2-032](#F-t2-032) |

Both browser references are ordinary browsers first: the user navigates anywhere, keeps profiles, sees history, and the agent is layered on top with a visible control indicator and a chat beside the page [REA/SRC]. Verevon has the agent layer (profiles, live view, AI/Manual chip, DevTools drawer) but not the ordinary browser underneath: every user session is an agent run with allowed_domains set to the first host, max_steps 12 and max_runtime_s 120 (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:483-490), the interactive BrowserChrome is mounted only in the dashboard Crawl composer (apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeScrapePreview.tsx:253), and the only history is a browser-local list capped at 10 entries (apps/Frontend Plane/verevonv3/src/features/dashboard/home/web-history.ts:32). Takeover is recorded server-side, but Quarry's step handler never reads control_mode (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:2227-2294).

### 2.9 AI-browser control

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Authenticated agent-to-browser control channel (agent loop outside the browser) | 2 of 3 (Aside, Codex) | Partial | [F-t2-001](#F-t2-001) |
| Browser-level command catalogue with audited tab/window/bookmark management | 2 of 3 (Aside, Codex) | Partial | none needed (Quarry sessions are server-side; no user-browser state to manage) |
| Code-mode browser control (model writes a script against a typed runtime) | 3 of 3 (Aside, Codex, Firecrawl) | Planned | [F-t2-063](#F-t2-063) |
| Sandbox hardening for model-written browser code and approval inputs | 2 of 3 (Aside, Codex) | Planned (no present gap: typed actions are plain data) | [F-t2-063](#F-t2-063) |
| Capability- and environment-conditional tool docs and policy delivered as data | 2 of 3 (Aside, Codex) | Not assessed (no baseline record; item Partial) | [F-t2-032](#F-t2-032) |
| Browsing operating procedure in the system prompt | 2 of 3 (Aside, Codex) | Partial (one-paragraph prompt; LLM planner off by default) | [F-t2-010](#F-t2-010) |
| Accessibility-tree observation with stable element refs and diffs | 3 of 3 (Aside, Codex, Firecrawl) | Shipped | none needed |
| Stale-target protection and ref re-resolution | 2 of 3 (Aside, Codex) | Shipped | none needed |
| Actionability checks, hit-target verification and trusted input events | 2 of 3 (Aside, Codex) | Partial | [F-t2-015](#F-t2-015) |
| Post-action settle logic and ambient page-event notes | 2 of 3 (Aside, Codex) | Partial | [F-t2-034](#F-t2-034) |
| Multi-path input with visual fallback and set-of-marks screenshots | 2 of 3 (Aside, Codex) | Partial | [F-t2-034](#F-t2-034) |
| Playwright-compatible semantic locators across frames | 2 of 3 (Aside, Codex) | Shipped | none needed |
| JavaScript dialog handling as controllable objects | 2 of 3 (Aside, Codex) | Shipped | none needed |
| Agent tab ownership and turn-scoped tab lifecycle | 2 of 3 (Aside, Codex) | Partial | [F-t2-013](#F-t2-013), [F-t2-054](#F-t2-054) |
| Co-browsing: agent acts in the tab the user is looking at | 2 of 3 (Aside, Codex) | Absent (a Model Plane run never attaches; item Partial) | [F-t2-013](#F-t2-013) |
| User takeover, manual handoff and interruption | Codex and Aside show takeover UI; verification found no reference that enforces a took-over state in code, so no count is given | Partial | [F-t2-013](#F-t2-013) |
| Durable human-in-the-loop approvals with fail-closed semantics | 3 of 3 (Aside, Codex, Firecrawl) | Shipped | none needed |
| Risk-tier confirmation taxonomy and final-confirm mode | 2 of 3 (Aside, Codex) | Shipped | none needed |
| Session permission modes and rule engine with action classes | 2 of 3 (Aside, Codex) | Partial | [F-t2-032](#F-t2-032) |
| Gated uploads and downloads with one-time grants | 2 of 3 (Aside, Codex) | Partial (code exists; driver capability flag is false) | [F-t2-039](#F-t2-039) |
| Secure credential handoff: the model never sees secrets | 2 of 4 (Aside; Codex cloud browser only), corrected to a differentiator | Absent | [F-t2-024](#F-t2-024) |
| Prompt-injection guard for web content | 3 of 3 (Aside, Codex, Firecrawl) | Partial (Model Plane screens; Quarry does not label) | [F-t2-049](#F-t2-049) |
| Visual proof and recording of agent work | 2 of 3 (Aside, Codex) | Shipped (no video) | none needed |

This is the category where Verevon is closest to the references and in places ahead: fail-closed target repair, cross-process frame actions, dialogs as objects, risk gates enforced in code, and immutable receipts are all shipped. The gaps are in the loop around those primitives. Ref-targeted clicks are synthetic element.click() calls with no visibility, stability or hit-test check (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:1232-1330), observation is captured immediately after an action with no settle (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs:576-640), the LLM planner is off unless QUARRY_BROWSER_AGENT_LLM is set (apps/Model Plane/rust/services/execution-core/src/llm_planner.rs:130-148), and no secret-reference action exists, so a login today needs the model to type the secret or the user to take over (apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs:638-700). The downloads path is the clearest "built but not wired" case: quarantine, caps and admission checks exist, but capabilities() reports downloads_to_artifacts=false, so every download_ref is rejected (chromiumoxide.rs:1877; agent_routes.rs:1191-1193).

### 2.10 Integration-plane overlap

No capability in this category is table-stakes across the references; its clusters are differentiators (provider-API routing, connector hints by domain, OTP via mail connectors) and are covered by [F-t2-024](#F-t2-024), [F-t2-043](#F-t2-043), [F-t2-056](#F-t2-056) and [F-t2-064](#F-t2-064) in the differentials section.

