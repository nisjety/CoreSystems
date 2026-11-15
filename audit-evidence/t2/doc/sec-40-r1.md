## 5. Recommended

Recommended items close the parity gaps that matter once the Priority tier has made the in-app browser and the Quarry acquisition API safe to run in production: capacity and observability, honest handling of blocked sites, page-to-chat context, consent and approval flows, monitors, webhooks and durable sessions. Each card states the status of Verevon at commit afb83928 (2026-10-10) as verified in code; effort and approach come from that verification pass, not from the original comparison notes. Cards appear in recommended build order within the tier.

<a id="F-t2-025"></a>
### F-t2-025 · Per-org capacity: concurrency caps with backlog and fair share, per-class rate limits and plan-aware quotas
*Verevon today: Partial · Effort: L · Plane: Ingestion Plane + Frontend gateway + Control Plane (entitlements) · Wave: W2 · Confidence: High*

**What and why.** Give every org a concurrency cap with a bounded backlog and fair-share priority, class-aware rate limits on the gateway's browser, scrape, search and agent routes, and limits keyed to the org's plan. Today one tenant can take the whole browser lease pool and agent browser runs have no cap at all.

**Reference practice.** Firecrawl is the best source, 1 of 4 references for the backlog mechanism: over-limit work waits in a per-tenant backlog with an expiry, the oldest entry is promoted when a job finishes, and 429 is returned only when the backlog is full [SRC]. Plan-scaled limits are 2 of 4 references: Firecrawl multiplies per-endpoint base limits by a plan factor with org overrides [SRC], and Apify documents plan concurrency limits but not the mechanism [DOC].

**Verevon today and approach.** The gateway already limits every route with one 600 rpm class keyed by client IP (apps/Frontend Plane/verevonv3/apps/gateway/src/rate_limit.rs:50-58); Quarry has a process-global lease pool of 2 or 4 permits (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs:323,337) and the agent_runs map has no capacity check (agent_routes.rs:107,1639). Build four parts: org-keyed class limiters in the gateway using the mcp.rs pattern (S); per-org permits in RuntimeLeasePool plus a cap on agent runs, after threading org_id into the adapter acquire (M); backlog and fair share at the quarry-control jobs table, not the per-crawl postgres_queue (M); org-core quota keys with billing-core plan defaults read via a service credential (S-M).

**Dependencies and risks.** Needs a cross-plane grant for org-core and an ADR on fail-open versus fail-closed; backlog rows hold no URLs under ZDR (Zero-Data-Retention).

<a id="F-t2-026"></a>
### F-t2-026 · Per-operation browser deadlines, bounded tab attach and retry-once on a destroyed execution context
*Verevon today: Partial · Effort: S · Plane: Ingestion Plane (quarry-browser) · Wave: W1 · Confidence: High*

**What and why.** Give every Chrome DevTools Protocol (CDP) call in quarry-browser an explicit deadline from one constants table, wrapped by a helper that returns a typed Timeout naming the operation, and retry idempotent read calls once when the execution context was destroyed. Hung steps are the first thing a user watching the live view notices, and they waste Model steps on transport errors.

**Reference practice.** Codex is the only reference with a per-operation deadline table, 1 of 4, though every reference bounds work somehow [SRC]. Codex wraps each CDP call with a constant deadline (about 20 s per command, 15 s page-ready, 5 s capture), rejects a tab attach after a 15 s handshake with distinct causes, and retries once on a context-destroyed error after dropping its cached isolated world, never retrying semantic errors [SRC].

**Verevon today and approach.** Explicit deadlines exist only for downloads and the 5 s profile capture (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:76-77,683-693); the BrowserConfig builder sets no timeout (226-262), so other calls ride the library default of 30 s and fail as non-retryable DriverFailed strings (1988-2023). Set request_timeout and launch_timeout explicitly, add one cdp_call(op, fut) wrapper returning ErrorCode::Timeout with {op, ms}, wrap goto, and retry once only for content, AX snapshot and evaluate. Never retry ref or coordinate actions. Calibrate values from BrowserTelemetry, not Codex's numbers.

**Dependencies and risks.** Drop the Model Plane planner rule: execution-core already retries non-risky transient failures and deliberately excludes browser.act (permission/mod.rs:261-271). Per-run overrides may only tighten a Quarry-side maximum.

<a id="F-t2-027"></a>
### F-t2-027 · Honest bot-block handling: closed block taxonomy, non-signals, one recovery, a user-visible blocked state, and no stealth code
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane + Model Plane (planner rule) + Frontend Plane (UI state) · Wave: W1 · Confidence: High*

**What and why.** Separate strong bot-block signals (captcha widget, WAF interstitial, repeated challenge) from non-signals (bare 403 or 429, sign-in, consent, paywall, region block), enforce at most one low-risk recovery in Quarry, show a "site blocked automated access" state with take-over and try-another-source actions, and remove dead stealth code. Users get one truthful outcome and enterprise buyers a defensible no-evasion posture.

**Reference practice.** Codex is the best source, 2 of 4 references with Apify, whose academy supplies only a taxonomy because it recommends evasion [SRC; Apify DOC]. Codex classifies site-served evidence into {kind, confidence, evidence}, states that bare 403/429, timeouts, sign-in pages and paywalls are not bot detection, allows one short wait and one reload with no fingerprint or network change, then stops and tells the user, recording only the hostname [SRC].

**Verevon today and approach.** The classifier at apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/observation.rs:154-214 substring-matches text and emits every hit at 0.98 confidence with requires_escalation=true; execution-core's BrowserObservation (browser_agent.rs:171-186) and both planner prompts never see the signal; the gateway labels any 403/429 "site_blocked" (knowledge/quarry.rs:283-304). stealth.rs is dead, but TLS-impersonation escalation, driver and proxy rotation on block statuses and a "stealth" preset are live. Order: add the signal to BrowserObservation and planner text; tiered classifier; single recovery enforced in Quarry; rewrite classify_scrape_failure; decide the live evasion-adjacent paths while deleting stealth.rs; build the blocked state on the takeover chip (BrowserChrome.tsx:1228-1236).

**Dependencies and risks.** Needs the anti-bot posture ADR; hostname aggregates stay in memory for ZDR orgs.

<a id="F-t2-028"></a>
### F-t2-028 · Page context to chat: Quarry hit-test, @tab mentions, selection and region attachments, frame context menu
*Verevon today: Absent · Effort: L · Plane: Frontend Plane with an Ingestion Plane endpoint and a Model Plane provenance path · Wave: W3 · Confidence: High*

**What and why.** Let a user point at a tab, region or element in the in-app browser and ask about it: an @tab mention token, region crops, a Quarry hit-test endpoint that turns a frame point into an accessibility (AX) ref descriptor, and a right-click menu over the frame. The descriptors map onto the AX refs browser.act already uses.

**Reference practice.** Codex and Aside both ship page-to-chat context, 2 of 4 references [SRC]. Codex uses a tab mention {browserId, tabId, title, url} re-validated against the live tab on use and reported unavailable on mismatch [SRC]. Aside has the richest payload: text or region selections with element summaries, a preview image and per-element selector, role, text and href, kept per tab and cleared on navigation [SRC].

**Verevon today and approach.** None of it exists: the global composer deliberately has no "@" trigger (apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:3873-3880) and Quarry has no hit-test endpoint or geometry in its contracts. The substrate is there: AX nodes carry backendDOMNodeId (quarry-browser/src/chromiumoxide.rs:1165-1180), the tabs list returns {tab_id, title, url, active}, and snapshots carry a generation used to reject stale refs. Slices: a mention token resolved in the gateway via the Quarry tabs list, fail-closed, injected as a labelled untrusted page-context block (S-M, no Quarry change); region attach by cropping the streamed frame; a read-only hit-test endpoint using CDP DOM.getNodeForLocation (M); then the context menu.

**Dependencies and risks.** TrustClass derives from tool names only (model-gateway moderation.rs:204-240), so BrowserScraped provenance for attachments needs a new classification path; mentions never grant; attachments stay ephemeral under ZDR. Depends on [F-t2-001](#F-t2-001) and [F-t2-012](#F-t2-012).

<a id="F-t2-029"></a>
### F-t2-029 · Quarry /metrics recorder, per-run statistics and graceful drain, then a span vocabulary, memory-aware admission and alert rules
*Verevon today: Stub · Effort: L · Plane: Ingestion Plane + Control Plane observability stack · Wave: W2 · Confidence: High*

**What and why.** Install the Prometheus recorder in quarry-edge, serve /metrics on an internal listener with about 20 low-cardinality series, persist a RunStatistics object per run, drain browser leases on shutdown, then add memory-aware admission, a span vocabulary and alert rules. Quarry has no metrics today, so there are no SLOs, capacity numbers or alerts for the layer users and agents depend on.

**Reference practice.** Firecrawl is the best source for metrics and per-run statistics, 2 of 4 references with Apify: prom-client registries on every worker with semaphore active, waiting and hold series [SRC], while Apify persists Crawlee Statistics per run and offers threshold alerts [DOC]. Apify's AutoscaledPool samples CPU, cgroup memory and event-loop lag and refuses work above 0.8 load [DOC]. Codex spans carry only url.origin and snapshot size, with approval wait subtracted [SRC].

**Verevon today and approach.** The metrics crates are declared but inert and no /metrics route exists (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/telemetry.rs:6-14); no Prometheus job or rule covers Quarry (apps/Control Plane/observability/prometheus.yml:1-47). Graceful SIGTERM shutdown exists at the HTTP layer (main.rs:84-100) but drains no leases. Order: recorder and internal /metrics first (S), fixing the stale PLAN.md and GAP.md claims in the same change; RunStatistics in the Go run record (M); lease drain (S); memory-aware admission with a multi-Chromium pool, since a viewport change relaunches the single shared browser (M-L); alerting, which is new infrastructure because no Alertmanager exists (M).

**Dependencies and risks.** Span emission ships together with the ZDR span sampler in [F-t2-004](#F-t2-004) (its Phase 2, after the Phase 1 log filter), and spans are never emitted for ZDR runs without that sampler; labels never carry URLs or org ids; /metrics must not share the public listener.

<a id="F-t2-030"></a>
### F-t2-030 · Monitor change quality: normalised-content fingerprint, text/JSON diffs, tags and an optional goal-aware judge
*Verevon today: Partial · Effort: L · Plane: Ingestion Plane; Model Plane judge blocked · Wave: W3 · Confidence: High*

**What and why.** Compare monitors on a fingerprint of the readable main-content markdown instead of raw response bytes, return a paragraph diff, accept a tag so one URL can keep separate histories, and later add a goal-aware judge. Monitoring already ships in the UI, so ad and timestamp churn flagged as change is visible to users now.

**Reference practice.** Firecrawl is the only reference with this design, 1 of 4 [SRC]. It stores the last snapshot per (org, url, tag), normalises before compare, derives new, same, changed or removed, produces a unified diff and an optional schema-based JSON field diff, and runs a judge at temperature 0 that returns {meaningful, confidence, reason, changes[]} and fails open [SRC].

**Verevon today and approach.** The compare basis is a blake3 hash of raw bytes (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/pipeline.rs:524,696); the normalised text_fingerprint only lands in the event payload. Baselines save artifact_id None and the diff artifact is a one-line string (quarry-edge/src/change_routes.rs:555,573); diff_paragraphs has no non-test caller; the monitor create body has only url and preset (gateway monitoring.rs:275-283). Phase 1 (M): add a text fingerprint to NormalizedOutput and the run_page response, switch both monitor paths, re-baseline existing monitors, persist the previous markdown as an artifact, add a diff route and tag column, and filter notifications in quarry-control notifyOnChange (resources.go:286-300). Phase 2 is blocked: every Model Plane switch is off and the model-invoke cross-plane grant is granted:false.

**Dependencies and risks.** Previous content is stored for non-ZDR orgs only; the JSON diff depends on /v1/extract, off by default.

<a id="F-t2-031"></a>
### F-t2-031 · Tenant-scoped, SSRF-guarded outbound webhooks with per-job events, then completion-triggered follow-ups and generalised monitors
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane · Wave: W3 · Confidence: High*

**What and why.** Make quarry-control webhooks tenant-safe before any customer can register one: org_id on registrations and deliveries, org-filtered fan-out, write-only secrets, a client guarded against server-side request forgery (SSRF), org-scoped edge routes and an optional per-job webhook on crawl and batch. Webhooks are how customers react to crawl and monitor results.

**Reference practice.** Apify is the best source for the contract, 2 of 4 references with Firecrawl: a rich event vocabulary, payload and header templates and ad-hoc per-run webhooks [DOC]. Firecrawl refuses private targets, supports wildcard event filters and metadata echo and keeps delivery logs, but delivers once with no retry [SRC].

**Verevon today and approach.** Delivery is already stronger than Firecrawl: timestamped HMAC signing, a 5s/30s/5m/1h ladder and a DLQ (apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/dispatcher/dispatcher.go:25-33,107-123), and run_completed is emitted with an idempotency key by every workflow (quarry-orchestrator/internal/workflows/workflows.go:124-148). But store.Webhook has no org_id and a json-tagged Secret (store/store.go:376-394), fanoutWebhooks lists webhooks across all orgs (resources/resources.go:612-650), the edge exposes no webhook routes, and the client is a plain http.Client (dispatcher.go:40-42). Phase 1: org scoping, a SafeClient built from a shared Go egress package copied out of integration-corev2 (its guard sits under internal/ of another module), edge CRUD and a tenant-facing delivery list. Phase 2 (follow-up actions, multi-target monitors) is a separate ADR-gated item.

**Dependencies and risks.** The guard and org filter ship now; public CRUD is demand-gated. Cross-plane reactions use NATS events or the owning plane's API, with idempotency keys and a chain-depth cap.

<a id="F-t2-032"></a>
### F-t2-032 · Per-origin agent consent and action-class grants: child grants, run modes, typed denials, org origin policy and a browser settings hub
*Verevon today: Partial · Effort: XL · Plane: Model Plane + Ingestion Plane + Control Plane + Frontend Plane · Wave: W3 · Confidence: High*

**What and why.** Add typed browser denials with a decision source, per-origin consent with turn, conversation and always scopes where any deny beats any approval, run modes and action classes on grants, a versioned org origin policy and a Settings > Browser hub. Verevon has the hard part (fail-closed risk gates, one-shot sensitive grants), but agents retry around generic refusals and users re-approve the same site repeatedly.

**Reference practice.** Codex is the best source: per-origin consent and typed denials are each 1 of 4 references [SRC]. Codex checks denies before approvals per origin and class, merges admin and user policy field by field with deny precedence, and wraps dispatch in one choke point returning {code, decisionSource, retryable, message} [SRC]. Action classes appear in Aside, where every Page method declares its class, 2 of 4 with Codex [SRC]; org-level origin policy also appears in Firecrawl, 2 of 4 [SRC].

**Verevon today and approach.** The broker host allowlist is fail-closed at run start, every step and in the CDP Fetch guard, but the grant store is in memory (apps/Model Plane/go/services/browser-broker/cmd/main.go:63), child grants cover three sensitive actions only (grant.go:32-34), status_err wraps a 403 as BAD_REQUEST (apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:1106-1122), and no origin policy or browser settings section exists. Split: typed DenialReason with decision_source modelled on CrawlDenialReason (S-M); per-origin consent via a new parent-bound origin-extension grant kind, since validate_run_grant demands run domains equal the parent at every step, with decisions persisted in Control Plane user settings (L); org policy and the hub, intersected by the broker at issue time (L).

**Dependencies and risks.** Needs [F-t2-001](#F-t2-001) and the web-policy-of-record ADR; no wildcard patterns until isCanonicalDomain and BrowserEgressPolicy change together; ZDR orgs limited to turn scope.

<a id="F-t2-033"></a>
### F-t2-033 · Approval loop UX: keyboard-first approval card with redirect, persisted site rules and attention notifications
*Verevon today: Partial · Effort: L · Plane: Model Plane (schema and rules) with Frontend Plane and Application Plane delivery · Wave: W3 · Confidence: High*

**What and why.** Turn the browser approval gate into a loop: one shared keyboard-first card in chat and the browser panel, a "do this instead" text delivered to the planner as a redirect, persisted org-level site rules, and approval_requested notifications that pull a user back to a background run. Today the card is click-only, repeats the same question and cannot be answered outside the browser panel.

**Reference practice.** Approval prompts and attention notifications each appear in 2 of 4 references (Codex, Aside) [SRC]. Codex defines the contract: display-friendly params, risk level, persist session or always, origin and page URL, with free text that rejects as "user asked to do X instead" and denials deep-linked to the setting [SRC]. Aside reports surface focus with a heartbeat and skips notifying for visible, disabled or ephemeral sessions [SRC].

**Verevon today and approach.** BrowserApprovalCard is approve or reject only (apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:322-395), chat has a separate ApprovalRequests deck, a denial aborts the run by design in gate_risky_action (apps/Model Plane/rust/services/execution-core/src/browser_agent.rs:870-945), and notification-core authorizes only run_completed and run_failed (apps/Application Plane/notification-core/internal/http/handlers.go:199-212). Slices: one shared card with shortcuts via use-chat-shortcuts.ts and redirect text in the existing decision reason field (S-M); richer approval creation on pause_for_approval and a planner path that takes redirect text instead of aborting; org-level rules in capability-core policy, not BrowserBroker, whose grants are in-memory (M); an approval_requested type on the runwatch path and notification-core allowlist (M).

**Dependencies and risks.** Depends on [F-t2-001](#F-t2-001) and [F-t2-032](#F-t2-032); sensitive categories stay single-use; Escape is already globally bound; notification bodies are title-only under ZDR.

<a id="F-t2-034"></a>
### F-t2-034 · Agent observation quality and budget: post-action settle, frame-aware AX snapshot with warnings, set-of-marks screenshots, capped projection and image budget
*Verevon today: Partial · Effort: M · Plane: Ingestion Plane + Model Plane · Wave: W3 · Confidence: High*

**What and why.** After each agent action, wait for commit, first paint and a short quiet window with a hard cap; add per-frame deadlines and a warnings list to the accessibility (AX) snapshot; prioritise truncation by viewport and interactivity; forward console and network summaries to the planner. UI sessions are capped at 12 steps, so every wasted observation is expensive.

**Reference practice.** Settle and frame-aware snapshots are table-stakes in both browser references, 2 of 4 (Codex, Aside) [SRC]. Codex awaits navigation commit and first paint (5 s caps) then AX and DOM mutation silence for 50-100 ms within 750 ms, and stitches per-frame AX trees under their IFRAME owner with per-frame timeouts inside a global deadline, collecting {frameId, error} warnings [SRC]. Aside samples mutation ratios over 350 ms windows, tracks same-site requests up to 1.2 s and queues page events into the next tool result [SRC].

**Verevon today and approach.** Quarry already returns a bounded AX and frame projection (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:1086-1128) with console and network summaries, but observation.rs:576-640 captures state immediately after the action with no settle, the AX fetch is one root-frame call with no per-frame deadline (chromiumoxide.rs:2681-2722), and the only settle is scrape-only (quarry-runtime/src/browser_driver.rs:28-70). On the Model Plane, observation_from_wire drops the summaries (execution-core/src/quarry_agent.rs:405-474) and the planner is text-only. Build the agent-action settle with per-action budgets and same-site filtering, reported as settle_ms; add warnings[] with serde defaults; align the 50-target cap with the planner's 40; carry page events to the model.

**Dependencies and risks.** Defer set-of-marks and the image budget: no planner receives pixels, and ScreenshotAnnotated already means a changed-region overlay. Side bug: observation.rs:2140 slices a byte index and can panic on a multi-byte boundary.

<a id="F-t2-035"></a>
### F-t2-035 · Durable hosted browser sessions: idle and absolute TTLs, suspend/resume, view vs control tokens, optional recording, scrape-to-session replay
*Verevon today: Partial · Effort: L · Plane: Ingestion Plane; scope handling in the Frontend Plane gateway · Wave: W3 · Confidence: Medium*

**What and why.** Extend Quarry's run durability with idle and absolute time-to-live (TTL) values and a reaper, a wider checkpoint state machine with suspend and resume, a working caller for resume_run_id, idempotent create, view-only scope on live-view routes, optional recording, and later a replay context on scrape results. Two minutes is short for real browsing and the roadmap's crash-restart rehydration test is still outstanding.

**Reference practice.** Firecrawl's hosted sessions are the best source, 2 of 4 references with Apify for lifecycle and live view [SRC; Apify DOC]. A session row carries state, last activity, idle and absolute TTL and an idempotency key; a reaper transitions rows; suspend snapshots the profile and releases the browser; live view uses short-lived view and control URLs; recording is disabled under ZDR [SRC]. Replay from a scrape result is unique to Firecrawl, 1 of 4 [SRC].

**Verevon today and approach.** The durable pieces exist: a Postgres AgentRunCheckpoint with status Active or Closed (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs:25-90), a resume_run_id path that refuses ZDR (quarry-edge/src/agent_routes.rs:1308-1335), and Model Plane durable runs with pause and resume. Missing: RunEntry has no timestamps (agent_routes.rs:79-102), persistent_session.rs has no consumer, nothing sends resume_run_id (execution-core/src/quarry_agent.rs:182-192), and recording flags exist only for providers. Add created_at and last_activity with a reaper shared with [F-t2-006](#F-t2-006), widen checkpoint status, implement suspend as release plus persisted profile, wire resume into StartRunRequest with the rehydration test, and add view-only as a scope on the owner-checked routes rather than bearer URLs.

**Dependencies and risks.** No second run state machine beside session-core; cross-user sharing is an ACL decision for a separate ADR; resume needs a valid broker grant ([F-t2-001](#F-t2-001)).

<a id="F-t2-036"></a>
### F-t2-036 · Durable, tenant-scoped authority and one audit record for every permission and approval decision
*Verevon today: Partial · Effort: XL · Plane: Model Plane (browser-broker, execution-core) + Control Plane audit-core + Ingestion Plane quarry-control · Wave: W3 · Confidence: High*

**What and why.** Persist BrowserBroker grants with parent-child inheritance, scope quarry-control webhooks by org, and define one AuthzDecision audit event that approval decisions, grant validations and Quarry policy denials publish on the audit bus. This completes the evidence chain for "the Model proposes, Quarry executes or rejects" that compliance reviews ask for.

**Reference practice.** Least-privilege permissions appear in 3 of 4 references; a structured per-decision audit record is unique to Codex, 1 of 4 [SRC]. Codex emits one record per decision with decided_by, decision context, origin only and duration, recording only denials for per-navigation checks [SRC]. Apify issues scoped, expiring tokens inherited by runs and webhooks, and retries webhooks 11 times over about 32 hours with a dispatch-id header [DOC].

**Verevon today and approach.** The grant store is an in-process map (apps/Model Plane/go/services/browser-broker/internal/grant/grant.go:62-64) with no production AcquireGrant caller; the Webhook table has no org_id (quarry-control/internal/store/store.go:376-383); the browser decided event leaves decided_by empty (execution-core/src/browser_events.rs:129-151) and session-core publishes no approval event (session-core/src/audit_publisher.rs:604-632). The durable approvals table already records decided_by and decision_reason, so the audit gap is publication and schema, not capture. Split: durable, issued grants merged into [F-t2-001](#F-t2-001) on the existing parent_grant_id binding (L); webhook org scoping shared with [F-t2-031](#F-t2-031) (M); AuthzDecision as a new event name on the flat verevon.audit.v2 shape, first from session-core approvals, later from Quarry via a new publisher and subject grant (M). Evaluate and raw CSS are already refused by both facades, so that allowlist change is defense-in-depth and is tracked in [F-t2-002](#F-t2-002), not here.

**Dependencies and risks.** Audit-core rejects events whose subject tokens mismatch the payload; events carry origin only and ZDR tags; the webhook migration fails closed on unscoped rows.
