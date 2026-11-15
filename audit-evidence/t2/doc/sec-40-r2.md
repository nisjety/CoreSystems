<a id="F-t2-037"></a>
### F-t2-037 · Document parsing depth: PDF classification and Model Plane OCR routing, Office and e-book conversion, bounded parsing and a per-org parse cache
*Verevon today: Partial · Effort: L · Plane: Ingestion Plane + Model Plane (doc_intel) · Wave: W3 · Confidence: High*

**What and why.** Route PDF and Office bodies in the scrape and crawl pipeline to native text extraction, send only scanned pages to the Model Plane layout provider for optical character recognition (OCR), convert Office and EPUB files under byte and time budgets, and cache parses per org. Enterprise knowledge is mostly PDF; crawls that skip it leave the Data Plane incomplete, and unbounded parsing of untrusted binaries is an availability risk.

**Reference practice.** Firecrawl is the best source: PDF classification is 1 of 4 references (Firecrawl), Office conversion 2 of 4 (Firecrawl, Apify). Firecrawl classifies pages as text or scanned, converts text pages natively and sends only OCR pages to a layout service; its document crate unzips under part and total budgets, keys a sha256 parse cache and limits parsers with a semaphore [SRC].

**Verevon today and approach.** pipeline.rs has no content-type router (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/pipeline.rs:444-545), so lopdf extraction runs only in the answer fetcher (quarry-runtime/src/answer.rs:835-839); docx.rs has no caller and reads unbounded (quarry-transform/src/docx.rs:15-45), although docs/GAP.md:85 claims both shipped. imports-core already ships budgeted PDF and Office parsers (apps/Ingestion Plane/imports-core/app/parsers.py:140-215). Sequence: shared router with native lopdf in spawn_blocking under budgets (S, with [F-t2-009](#F-t2-009)); an ADR between porting imports-core's budgets and calling its ephemeral extract endpoint; OCR via /v1/ai/documents/layout only after a manifest row, grant and ZDR exception exist; then an (org, sha256) cache skipped under Zero-Data-Retention (ZDR).

**Dependencies and risks.** Quarry holds no Model Plane call grant today and model_hop_allowed refuses every hop under ZDR; OCR cost needs per-org page caps.

<a id="F-t2-038"></a>
### F-t2-038 · Agent presence and run control UX: animated cursor and target highlight, real annotation markers, steer/queue follow-ups, needs-attention notifications
*Verevon today: Partial · Effort: L · Plane: Frontend Plane + Model Plane + Application Plane · Wave: W3 · Confidence: Medium*

**What and why.** Show users what the agent is about to do in the shared browser, let them mark elements for it, redirect a run at the next step boundary, and notify them when approval is needed. These are daily-use expectations once the step loop is live, and today the product advertises an annotation feature that is a placeholder.

**Reference practice.** Each mechanism is 1 of 4 references. Codex moves a host-drawn cursor with a sequence number before every pointer command and sends annotation payloads with role, name, selector, rect and nearby text [SRC]. Aside sends idempotent WebSocket commands with steer{interrupt, expectedTurnId}, queue and afterSeq resume [SRC].

**Verevon today and approach.** browser-session.ts:127-137 advertises 'annotate' and returns one hard-coded anchor rendered at BrowserChrome.tsx:1822-1857; the gateway advertises the same (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:2578). Quarry's action.started payload is {step, action} with no geometry (quarry-core/src/contracts.rs:360-460). Runs have pause, resume and stop only (execution-core/src/browser_agent.rs:972-1011); chat-turn queueing exists in model-gateway queued_input.rs but not in the browser loop; run_completed and run_failed notifications already flow to notification-core. Four slices: remove 'annotate' and the placeholder (S); add an optional rect to Quarry snapshots via the CDP box model and animate it, presentation-only (M-L); steer and queue consumed by execution-core at the existing pause boundary with session-core replay (M); a needs_approval notification type plus watch toggle (S-M).

**Dependencies and risks.** Steering never bypasses a pending approval; queued input stays refused for ZDR tokens; Quarry's SSE ?from_seq is a no-op, so replay is Model Plane work.

<a id="F-t2-039"></a>
### F-t2-039 · Governed downloads and uploads: quarantined, tenant-scoped artifacts, a downloads drawer and hand-off to the Data Plane
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane + Data Plane (ingest API) + Frontend Plane · Wave: W3 · Confidence: High*

**What and why.** Prove, gate and surface the download path that already exists in the Chromium driver: agent downloads behind a single-use sensitive grant, unsolicited downloads cancelled, files stored as org-scoped artifacts with a scan hook, listed in a drawer and handed to the Data Plane. Today any page that serves a file is a dead end for agents.

**Reference practice.** Codex is the only agent-aware gate (1 of 4 references): its will-download handler cancels unless the download is user-initiated or consumes a stored grant matched on URL chain, session and expiry, and pushes a status machine to the agent [SRC]. Plain download handling is 2 of 4 (Aside, Codex) [SRC].

**Verevon today and approach.** ChromiumoxideDriver creates a per-session quarantine directory and a frame-bound, 25 MiB-capped download_from_verified_target (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:433-456, 2211-2390); the runtime refuses under ZDR, checks magic bytes and stores an org-stamped artifact (quarry-runtime/src/observation.rs:475-512). All of it is gated off by downloads_to_artifacts=false (chromiumoxide.rs:1877). The artifact store is already org-scoped (quarry-runtime/src/artifact_store.rs:100-120), so that dependency is dropped. Order: grant issuance from [F-t2-001](#F-t2-001) with allowed_actions=[download_ref]; tests and default-deny for unsolicited downloads; flip the flag; drawer and timeline events; Data Plane hand-off through the ingest client with a scan hook before widening admitted types. Uploads stay out of scope.

**Dependencies and risks.** Grant issuance is unbuilt (gateway browser.rs:1258-1262); admission refuses Office formats until a scanner exists; safety review before enabling.

<a id="F-t2-040"></a>
### F-t2-040 · Parallel batch scrape with shared options, idempotent creation and wait-or-resume
*Verevon today: Partial · Effort: L · Plane: Ingestion Plane (quarry-orchestrator, quarry-edge, quarry-control) · Wave: W3 · Confidence: High*

**What and why.** Fan out /v1/batch under a concurrency bound with shared scrape options, accept a client Idempotency-Key on job creation, and add a bounded wait that returns a resumable 408. Sequential batches are visibly slow for ingestion and agents, and retries from the gateway or Model Plane must not double-start crawls.

**Reference practice.** Firecrawl is the best source. Batch with maxConcurrency and append is 2 of 4 references (Firecrawl; Codex declared only); wait-with-resume 2 of 4 (Apify, Firecrawl); a client idempotency key 1 of 4, and Firecrawl's guard is a negative example [SRC; Apify DOC]. Firecrawl keys heavy requests by (org, request hash) and returns 408 processing_continues with Retry-After so an identical retry re-attaches [SRC].

**Verevon today and approach.** /v1/batch forwards only urls, ingest, org and user (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs:669-716) to a sequential BatchJobWF with no options or cancel handlers (quarry-orchestrator/internal/workflows/workflows.go:357-381). The handoff sends no Idempotency-Key (quarry-edge/src/handoff.rs:26-82), and control's unique index is on idempotency_key alone, so two orgs choosing the same key collide. No wait_ms or re-attach exists. Order: S quick win, accept and forward the key, namespace by org, index (org_id, idempotency_key), payload-hash 409; M, bounded-parallel BatchJobWF with [F-t2-005](#F-t2-005) options, appendToId, invalid-URL report and CrawlJobWF's pause and cancel handlers; M-L, per-job get and cancel routes ([F-t2-017](#F-t2-017)) then the 408 envelope.

**Dependencies and risks.** Durable jobs are refused under ZDR (routes.rs:563-583), so wait-or-resume is non-ZDR only; per-host politeness is already enforced in PageRunner.

<a id="F-t2-041"></a>
### F-t2-041 · Sitemap, robots and index-assisted crawl discovery, plus URL canonicalisation and redirect de-duplication
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane · Wave: W3 · Confidence: High*

**What and why.** Extend the existing sitemap and robots parsers into a discovery step that seeds live crawls (robots Sitemap lines, gzip, nested indexes) behind a sitemap include|only|skip option, and derive frontier identity from a canonical URL with a post-redirect check. Sitemap seeding makes crawls of docs and product sites complete and cheap; canonicalisation keeps duplicates out of the Data Plane.

**Reference practice.** Firecrawl is the best source; sitemap discovery is 2 of 4 references (Firecrawl, Apify), canonicalisation 1 of 4 (Firecrawl). Firecrawl queues robots, scoped, root and registrable-domain sitemaps in parallel, filters through the scope filter, locks the canonical URL permutation before enqueue and the redirect target after fetch [SRC]. Apify probes common sitemap names [DOC].

**Verevon today and approach.** quarry-transform parses sitemaps and robots Sitemap lines (apps/Ingestion Plane/Quarry-v2/crates/quarry-transform/src/robots.rs:80-81), and /v1/map reads /sitemap.xml plus five nested indexes but ignores robots lines and gzip (quarry-edge/src/map_routes.rs:132-250). Live crawl has no sitemap seeding (routes.rs:533-553) and dedupes on the exact link string (workflows.go:417). Do not add a unique_key column: the Postgres frontier already enforces (org_id, request_id) (quarry-runtime/src/postgres_queue.rs:243-260), so derive request_id from the canonical URL. Improve /v1/map first (S), then a discovery activity that enqueues into the durable frontier after the [F-t2-016](#F-t2-016) scope filter (M), then post-redirect reconciliation using final_url. Index seeding comes later.

**Dependencies and risks.** Discovered URLs are attacker-controlled and must pass the security preflight and robots filter; aggressive normalisation stays opt-in; never pass thousands of URLs through Temporal history.

<a id="F-t2-042"></a>
### F-t2-042 · Agent web toolset: content-hydrated search, next-step hints, citations, and Quarry jobs as approval-gated tools with cost estimates
*Verevon today: Partial · Effort: L · Plane: Model Plane + Ingestion Plane · Wave: W3 · Confidence: Medium*

**What and why.** Let one agent call return search hits with page content, add success-path hints from local signals, attach stable source ids to agent-path tool output, and expose crawl, map and batch as approval-gated tools with caps. Agents today can only search and fetch one URL, so the Model falls back to repeated single fetches.

**Reference practice.** Firecrawl and Codex are the best sources. Search with content is 2 of 4 references (Firecrawl, Apify); hints and documentation gating 2 of 4 (Firecrawl, Codex); citations with per-URL permission 1 of 4 (Aside). Firecrawl scrapes the top N concurrently inside a time budget and emits rule-based hints that never include page text [SRC]. Apify confirms estimated cost before a paid run [DOC].

**Verevon today and approach.** /v1/search has no scrape options (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/search_routes.rs:50-120); execution-core web_search sends only query, limit and zdr (apps/Model Plane/rust/services/execution-core/src/web_tools.rs:120-175); next_actions exists only on errors; no cap.web.* ids exist (execution-core/src/capability_policy.rs:775-835); the COST approval kind has no producer. The chat path already has citations, freshness guidance and untrusted-data labelling (model-gateway/src/tool_loop.rs:4304-4310, 4469). Split: (A) hydration via the /v1/answer fetch fan-out, hints on the retrieval_metadata.suggested_next_tools pattern, numbered source ids from the chat citation machinery (M); (B) crawl, map and batch as approval-gated tools with Quarry-enforced caps, cost-estimate approval once a cost source exists (L). Drop the docs-read gate; the MCP facade stays in [F-t2-064](#F-t2-064).

**Dependencies and risks.** [F-t2-005](#F-t2-005), [F-t2-040](#F-t2-040), [F-t2-008](#F-t2-008); hydration multiplies spend; org domain policy is a separate Control Plane item.

<a id="F-t2-043"></a>
### F-t2-043 · URL-aware planner guidance: site skills, integration-corev2 provider capability hints by domain, driver capability matrix
*Verevon today: Partial · Effort: L · Plane: Model Plane + Ingestion Plane (integration-corev2 catalog) · Wave: W4 · Confidence: High*

**What and why.** Give the planner routing guidance for web tools, let it see the driver capabilities Quarry already returns, inject operator site skills when the observed URL matches, and offer a governed provider action when the agent lands on a connected provider's site. Each change cuts rejected proposals, shrinks prompts and replaces brittle clicking with audited API calls.

**Reference practice.** Three mechanisms at 1 of 4 references each. Codex marks API members unsupported per backend and lets the agent list capabilities with one-line descriptions [REA, SRC]. Firecrawl ships per-endpoint skills, narrowest first, and matches provider tools to a URL host [SRC]. Aside's SKILL.md frontmatter carries autoInject.url, and matched site skills are injected bounded [SRC].

**Verevon today and approach.** web_search and web_fetch descriptions are one sentence with no web prompt snippet (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4184-4192); AgentSkill has trigger_file_patterns but no URL pattern (apps/Model Plane/proto/model_plane/v1/sessions.proto:876-893). Corrected premise: POST /v1/agent/runs already returns driver_capabilities and execution_tier (quarry-edge/src/agent_routes.rs:1180-1260), but nothing consumes them. integration-corev2 operations are string switch arms with no domains or schemas (integration-corev2/internal/actions/service.go:727). Four slices: rewrite descriptions plus a conditional SNIPPET_* web snippet (S); forward driver_capabilities into what the Model sees (S); url_patterns on AgentSkill mirroring trigger_file_patterns, matched on browser.observe (M); a versioned integration-corev2 operation catalog with provider domains replacing execution-core's CATALOG (L).

**Dependencies and risks.** Hints inform only; writes keep the approval path. Cap hints to avoid tool-list bloat; site skills stay inside the 8000-character budget; URL matching stays in memory.

<a id="F-t2-044"></a>
### F-t2-044 · Hermetic browser and scrape evals in CI through the production tool contract, with injection and ZDR fixtures and per-release scores
*Verevon today: Partial · Effort: L · Plane: Ingestion Plane + Model Plane (eval harness) · Wave: W3 · Confidence: High*

**What and why.** Run the existing eval binaries in CI with the commit SHA, then make the real Model Plane to quarry-edge loop test hermetic and scored, with prompt-injection pages and ZDR no-write assertions. Parity and safety claims stay honest only if evals exercise the production contract; today no browser agent eval runs anywhere.

**Reference practice.** Firecrawl is the only reference (1 of 4). It serves a fixture site in CI, runs end-to-end tests over a matrix of engines and backends, records driver exchanges for replay, and runs evals on a labelled PR and after deploy [SRC]. Codex reuses the same runtime archives in training and production [DOC].

**Verevon today and approach.** No workflow runs the lab/evals binaries (.github/workflows/quarry-v2-ci.yml:150-211), docs/SCOREBOARD.md:13-17 lists every suite as pending, and the browser benchmark is a mock-driver latency microbenchmark, not a task corpus (apps/Ingestion Plane/Quarry-v2/lab/evals/src/browser_benchmark.rs:1-12). TestSite has no consumer; canary.rs and experiments.rs are unwired scaffolding. execution-core has a real loop test with planner probe cases, ignored for needing a live stack (apps/Model Plane/rust/services/execution-core/tests/browser_agent_e2e.rs:1-60), and ZDR rejection is covered by quarry-edge/tests/zdr_behavior.rs. Phase 0 (S): a CI job running quarry-eval, bakeoff and release_matrix with the SHA. Phase 1 (M): make browser_agent_e2e hermetic against TestSite with a stub grant validator and a deterministic or recorded planner, score success, steps and cost, add injection pages asserting on the Model Plane screening verdict, and ZDR no-write assertions. Drop the mirror-and-diff clause.

**Dependencies and risks.** Injection screening lives at the Model Plane render seam (execution-core/src/provenance.rs); Chromium jobs belong in a labelled or scheduled workflow given documented CI instability.

<a id="F-t2-045"></a>
### F-t2-045 · Memory pollution guard for runs that ingested web or browser content (verify first)
*Verevon today: Absent · Effort: M · Plane: Model Plane · Wave: W2 · Confidence: High*

**What and why.** Taint a run or thread window once a web, browser or MCP tool result enters context, and make save_memory and Dreaming extraction skip or quarantine it. Otherwise a prompt injection read from one page can become a persistent memory injected into every later session.

**Reference practice.** Codex is the only reference (1 of 4), with a no_memories_if_mcp_or_web_search setting and a polluted thread memory mode: a flag set when such a result is added, which the extraction job checks to skip or route candidates to review [SRC, medium confidence].

**Verevon today and approach.** No taint or memory_mode flag exists in any plane. execute_save_memory takes no taint (apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs:650-663) and save_memory is deliberately ungated (execution-core/src/permission/mod.rs:171-182); Dreaming upserts review_state 'accepted' unconditionally (apps/Model Plane/rust/services/session-core/src/dreaming.rs:1636-1690). Mitigations exist: the extractor treats conversation as untrusted and reads only user and assistant rows (session-core/src/dream_extractor.rs:616-640), but an assistant turn paraphrasing a poisoned page is still extracted. The proposed ZDR carrier does not fit, since MemoryRetention is credential-bound (session-core/src/grpc.rs:4096-4150) while taint appears mid-run. Instead: execution-core save_memory preflight refuses or marks the write when the run's tool history holds web, browser or mcp__ calls; Dreaming writes 'pending' or skips windows whose thread has such steps, derived from session_tool_audit_intents, and suppresses the Letta mirror; ship skip-only first (S, quick win). Drop the Data Plane clause; no agent-derived graph write path exists.

**Dependencies and risks.** Taint must be thread-window level; the pending path needs a new accept/reject RPC, and only the memory owner may promote.

<a id="F-t2-046"></a>
### F-t2-046 · Complete web-usage metering: browser metrics, outcome and surface attributes, price table and plan keys
*Verevon today: Partial · Effort: L · Plane: Control Plane (pricing, entitlements) + Ingestion Plane (metrics, concurrency) + Frontend Plane (rate-limit classes, usage UI) · Wave: W3 · Confidence: High*

**What and why.** Meter browser steps, minutes and agent runs, give extract and map their own metrics with outcome and surface attributes, add browser plan keys in billing-core, and put a per-org fair-share cap in front of the browser lease pool. The most expensive Quarry feature is invisible to billing, and one org can consume every lease.

**Reference practice.** Firecrawl is the best source and metering is table-stakes at 3 of 4 references (Firecrawl, Apify, Aside). Firecrawl prices each job through one pure function with failures free, surcharges for LLM, PDF pages and ZDR, two limit units and a closed origin tag per request [SRC]. Apify meters compute units; Aside gates features per plan [DOC; Aside SRC].

**Verevon today and approach.** Quarry emits five idempotent metrics over NATS (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/usage.rs:50-94), but extract and map are billed as search queries and extract meters every attempted target (quarry-edge/src/extract_routes.rs:345-361); agent_routes.rs:700-761 emits no UsageEvent, leaving only a flat action_cost receipt for budgets. The scrape event puts the final URL into metadata even under ZDR (quarry-edge/src/routes.rs:490-505). No browser plan keys exist (apps/Control Plane/billing-core/internal/billing/defaults.go:5-80) and leases are two fixed process-wide pools (quarry-runtime/src/lease_pool.rs:30-45). Derive the surface tag from the verified principal, split extract and map and meter successes only, emit step and minute events with ids and counts only under ZDR, fix the metadata leak in the same change, put plan keys in billing-core behind the leads-style entitlement gate, and add a per-org semaphore before RuntimeLeasePool, the same per-org permits as [F-t2-025](#F-t2-025), built once.

**Dependencies and risks.** Prices never live in Quarry; each metric needs a Lago code first; confirm which key the gateway limiter uses before adding per-class buckets.

<a id="F-t2-047"></a>
### F-t2-047 · Fetch-path reliability: one deadline budget, wired retries and typed failure sub-codes
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane (quarry-runtime, quarry-core, orchestrator); Frontend Plane gateway for classification · Wave: W3 · Confidence: High*

**What and why.** Add typed failure sub-codes, a caller-visible timeout split across the driver waterfall with child cancellation, and in-process retries on the synchronous scrape path. Escalation through static, TLS and browser drivers should finish inside one deadline, and the Model and UI need to know why a fetch failed so they can switch source, wait or ask.

**Reference practice.** Firecrawl is the best source for abort tiers and error vocabulary; retries are 2 of 4 references (Apify, Firecrawl), deadline budgeting and transportable codes 1 of 4 (Firecrawl). Firecrawl composes request, scrape-deadline and per-engine cancellation tokens and serialises one error union across queues and webhooks with a processing_continues detail [SRC]. Crawlee retries per request with error and failure hooks [DOC].

**Verevon today and approach.** execute_with_retry is called only by its own tests (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs:164); live retries are Temporal's five attempts on job paths (quarry-orchestrator/internal/workflows/workflows.go:94-104). FallbackDriver walks drivers with no budget (quarry-runtime/src/fallback_driver.rs:57-120), ScrapeRequest has no timeout (quarry-edge/src/routes.rs:346-375) and FetchPolicy.timeout_s is read nowhere. DriverFailed is not retryable (quarry-core/src/error.rs:38-43), so wiring the helper alone does little. Reorder: sub-codes first (DriverFailed{dns|tls|connect|all_drivers_failed} with their own retryable flag; robots and policy denials non-retryable); then timeout_ms clamped by Quarry with per-attempt child tokens and a reserved slice for the last tier; then retries around the sync scrape path only, re-entering the per-host slot and reconciling the two retry config types.

**Dependencies and risks.** In-process retries on job paths would multiply Temporal attempts; model-gateway already bounds scrapes at 30 s; propagating sub-codes across planes raises effort to L and needs a versioned schema.

<a id="F-t2-048"></a>
### F-t2-048 · Org-scoped URL cache with explicit freshness, and a cache-only no-egress mode
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane; Data Plane for the artifact-backed no-egress path · Wave: W3 · Confidence: High*

**What and why.** Replace the fingerprint-keyed page cache with an (org, normalised URL, variant) cache honouring maxAge, minAge and store flags, reporting cacheState and cachedAt and caching misses; later add a no_egress mode that serves only from cache and Data Plane artifacts. Repeated research gets cheaper, fewer requests hit target sites, and regulated workspaces get a hard no-outbound guarantee.

**Reference practice.** Firecrawl is the only reference (1 of 4). Its key is a hash of the normalised URL plus a variant, with a metadata index and Redis hot path; reads resolve maxAge and return cacheState and cachedAt, negative windows skip the database, and lockdown rewrites the engine plan to index-only, forces ZDR and returns a typed miss without logging the URL [SRC].

**Verevon today and approach.** CachePolicy and should_read/should_write are live and bypassed under ZDR (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs:56-76, 430-440), but the Redis PageCache is keyed by content fingerprint with no org segment and read only when the caller supplies prev_fingerprint (quarry-edge/src/cache.rs:1-30); the URL-plus-vary helper is dead scaffolding (cache.rs:84-105), the preset SWR field has no consumer, and the gateway keeps a URL-only-keyed preview cache (apps/Frontend Plane/verevonv3/apps/gateway/src/cache.rs:80-125). Simplify phase 1: keep Redis and the cached NormalizedOutput value, key through the existing helper with an org prefix mirroring SearchCache::key, add the freshness fields additively, implement or delete stale_while_revalidate_s, fix org scoping of the current cache in the same change (a caller with another tenant's fingerprint may read its output; verify) and retire the gateway copy. no_egress is phase 2 behind the [F-t2-022](#F-t2-022) policy floor.

**Dependencies and risks.** Never cache credentialed, cookie, profile or action requests; no_egress reaches Data Plane artifacts only through its API and returns a typed miss without logging the URL.
