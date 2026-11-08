## Appendix C. Reference-side verification log (Priority items)

Every Priority recommendation was re-checked by an independent skeptic that re-opened the cited evidence in the reference apps, repos or public docs and tried to refute it. This log records what that check corrected. The cards in section 4 already incorporate these corrections.

### [F-t2-001](#F-t2-001): Production BrowserBroker grants and delegated bearer forwarding (unblocks every browser path)
Reference-side check: **partial** (confidence high).

**Corrections:** 1) Prevalence/class: the cluster C-t2-08-001 mechanism (will-attach-webview rewrite) is Electron-specific; Aside being a Chromium fork does not imply it has the same attach hardening, so the 'treated as common because Aside is a Chromium fork (INF)' line must go. State '1 of 3 (Codex), unique' for the attach hardening; 'real Chromium engine' alone is 2 of 3. 2) Best-in-class fit: Codex webview hardening is not an analogue of grant issuance. The closest reference mechanisms are Aside's signed-challenge short-lived CDP token plus routed bridge (SRC) and Codex's signed, nonce-bound per-turn browser/access check (SRC, CA028). Both are single-user local-device trust checks, not multi-tenant durable grant stores. Say plainly that no reference implements a durable org-scoped grant issuer; the issuer is a Verevon design requirement and not a parity item. 3) Firecrawl: the session timers are enforced by the external Hangar browser service (Firecrawl sends max_lifetime_seconds and idle_timeout_seconds; Hangar is not in the repo). Firecrawl's own code runs a 15 s reconciler that settles billing. It is not a reaper that closes sessions on either timer. Reword 'a reaper closes a session on eit…

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Firecrawl browser sessions depend on Hangar (HANGAR_URL, hosted and closed source); TTL enforcement is INF from the parameter names and not observed; Firecrawl /interact needs USE_DB_AUTHENTICATION and is rejected for Safe Mode orgs (per record notes; not re-read); Codex per-turn browser access is tied to a 'Work settings' toggle; enterprise or workspace gating is possible (INF from the error string); Aside evidence comes from a scratch copy of the aside-daemon.exe NODE_SEA_BLOB (SRC by byte offset), not from a source repo

### [F-t2-002](#F-t2-002): Gate Evaluate and raw script actions behind a grant capability and one-shot approval
Reference-side check: **partial** (confidence high).

**Corrections:** Codex does not put playwright.evaluate behind a capability or an approval. It makes evaluate structurally safe instead: an isolated world, read-only wrappers, masked credential fields, a block on import(), and capped results. The consent and approval gates apply only to raw CDP: the full_cdp_access_enabled preference (default false), the enterprise requirement, and a per-origin consent that a preference can auto-answer. 'Domain allowlist' means CDP protocol domains (22 usable), together with a per-method denylist and parameter checks, not website domains. 'grant rawCdp=true' and 'one-shot approval' are Verevon proposals, not Codex mechanisms. 'Unique' holds only for the read-only evaluate. Gating raw script actions is also in Firecrawl, where Safe Mode strips executeJavascript and rejects interactive sessions, and Aside isolates the REPL guest from the raw CDP client, although Aside's page.evaluate can mutate the page subject only to permission rules.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Codex raw CDP is default-off and gated by preference and enterprise requirement; the source record says the docs mark it development-only (doc not re-opened); Codex evidence comes from a beautified copy of the shipped browser plugin bundle (SRC of a shipped artifact)

### [F-t2-003](#F-t2-003): Close the gateway search-cache ZDR and tenant hole, then publish a per-feature ZDR capability matrix
Reference-side check: **partial** (confidence high).

**Corrections:** shouldCache is an inline boolean inside sendDocumentToIndex, not a separate pure function. It excludes any custom headers, actions or profile (hasCustomRequestContext), not only credential-bearing input, and there is no 'session' term. It also excludes results from low-fidelity engines (TLS client, fetch, except the sitemap team), Exchange, the index itself, and page-aware or marker PDF parsers. Firecrawl's deletion is a per-request ZDR cleanup queue plus access-row expiry (24 h default, crawlTtlHours for crawls), not a janitor that runs on plan retention. Plan-based retention comes from Apify and is DOC-only. 'Run history with retention 3 of 4' is overcounted: the Aside record cited (065) shows local session storage and no retention policy. Aside's only evidenced retention is a 15-minute purge of ephemeral sessions in a terminal state. Several ZDR refusals (agent/extract, scrape-status) trigger when the team flag forces ZDR, not on the per-request flag.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Apify retention is DOC-only; Firecrawl ZDR forced/allowed modes are enterprise team flags; The retention behaviour of every ZDR sink was not traced, as the source record itself notes

### [F-t2-004](#F-t2-004): ZDR-safe telemetry: span sampler, late-ZDR registry, log filter and safe/sensitive log fields in Quarry and the gateway
Reference-side check: **partial** (confidence high).

**Corrections:** Prevalence conflates two different things: ZDR-aware telemetry is Firecrawl-only (Codex's BROWSER_USE_DISABLE_AMBIENT_NETWORK is a sandbox/eval kill switch for gates, Sentry and analytics, not a ZDR mechanism), and safe/sensitive classified log fields are Codex-only (Firecrawl filters whole log lines by a zeroDataRetention flag). State 1 of 4 for each, both differentiators. The late-ZDR registry is a Set with oldest-first (FIFO) eviction, not an LRU. 'Helper services' is one service in source (Go html-to-md); only that client sends X-Zero-Data-Retention. Codex's safe/sensitive split is observed at call sites and in the logger shape, but how sinks treat 'sensitive' (drop/hash/upload) was not found, so 'sinks drop or hash sensitive' is a Verevon proposal (INF), not Codex behaviour. The {safe, sensitive} convention is app-wide in Codex, not limited to browser lifecycle logs. Firecrawl's implementation is TypeScript OpenTelemetry; the Rust Sampler/SpanProcessor/tracing Layer wording is a port, not observed.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Firecrawl tracing is off unless an OTLP endpoint is configured; the ZDR sampler matters only when tracing is enabled; Codex evidence is from the shipped minified bundle (SRC via extracted copy); sink behaviour for sensitive fields unverified; BROWSER_USE_DISABLE_AMBIENT_NETWORK is an environment switch in the browser-use plugin runtime, not a user-facing privacy setting

### [F-t2-005](#F-t2-005): One strict ScrapeOptions contract: formats[], inline return (the ZDR path) and main-content controls
Reference-side check: **partial** (confidence high).

**Corrections:** (1) 'Write none for ZDR' is a Verevon design choice, not what the reference does. Firecrawl still writes the job-access row and tags it with zeroDataRetention. It refuses GET /scrape/:id only for teams forced into ZDR. (2) The described transformer order does not match the code. The actual order is html, markdown, LLM clean (onlyCleanContent), redactPII, prompt-injection guard, links, images, branding, metadata, product/menu, LLM json/deterministicJson, summary, query, attributes, agent, removeBase64Images, diff, audio/video, then coerceFieldsToFormats. So redaction runs early, not after the LLM formats, and every transformer runs and checks its own option before the final coercion strips unrequested fields. Requested transformers are not the only ones run. (3) The 'choice of extractor with fallback to full HTML' comes from Apify (DOC), not Firecrawl, and Apify ties the fallback to whether the page is a clear article, not to a text-length threshold. Firecrawl uses fixed selector heuristics. (4) Apify Web Fetch does not have 'the same format list': it has only markdown/html/text/links/raw, with no screenshot, json, attributes or images.

**Caveats:** Firecrawl parts are SRC; the Apify Web Fetch and Website Content Crawler parts are DOC-only; The skipTlsVerification=true default is a real footgun in the reference; do not copy it; onlyCleanContent is an LLM pass that is unsupported under ZDR and looks experimental; Firecrawl's openapi.json lags the code (formats are missing), so cite the code, not the spec

### [F-t2-006](#F-t2-006): Reap abandoned agent runs (idle and absolute TTL), cap concurrent runs, and make the fleet console real
Reference-side check: **partial** (confidence high).

**Corrections:** Aside's sweeper (flushIdleTabs) closes agent-owned tabs only for sessions whose status is idle, interrupted or aborted and whose session updatedAt is older than closeAfterIdleMinutes. It skips sessions with live activity and uses the session timestamp, not a per-tab last-activity timestamp. It does not reap running runs or delete sessions, and no event emission was seen. Ephemeral sessions in a terminal state are purged after 15 minutes. Aside has no absolute run TTL. A separate in-memory idle TTL of 10 minutes (DEFAULT_IDLE_TTL_MS) unloads agent session objects. The 5-tab warning is a text line added to the tool output. Prevalence is understated: Firecrawl hosted browser sessions have both an absolute ttl (default 600 s, max 3600) and an activityTtl (default 300 s), enforced by the closed Hangar service. Firecrawl is the better reference for 'idle and absolute TTL' in a multi-tenant server. No reference evidence supports a 'fleet console'.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Aside limits are constants in a local single-user daemon (SRC from the SEA blob); Firecrawl TTL enforcement happens in the closed Hangar service; only the request params and their pass-through were seen; maxBrowserSessions enforcement not found

### [F-t2-007](#F-t2-007): Stop viewport-triggered browser relaunches: per-page emulation, a small Chromium process pool and page hygiene
Reference-side check: **partial** (confidence high).

**Corrections:** 1) Firecrawl does not run a browser process pool: the OSS playwright service launches ONE Chromium and bounds concurrent pages with a semaphore; no retirement, relaunch or placement. Pool lifecycle is 1 of 4 (Apify Crawlee BrowserPool); Firecrawl contributes only the page semaphore and health probe. 2) Crawlee assigns a new page to the FIRST active browser below maxOpenPagesPerBrowser (not the least-loaded); retired browsers close when they have no pages or via the inactivity sweeper (closeInactiveBrowserAfterSecs 300 s). 3) Firecrawl /health creates a fresh context and blank page and closes them; it does not navigate to about:blank and has no explicit deadline. 4) The Rust Vec<BrowserSlot>/Semaphore shape is a Verevon proposal (INF), not a reference mechanism.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Apify BrowserPool evidence is DOC plus public OSS source (not an installed artifact); Aside page-hygiene CDP calls are SRC from the daemon SEA blob and conditional on an internal flag; Codex throttling exemption is for the Electron desktop shell (webContents.setBackgroundThrottling), not a headless pool

### [F-t2-008](#F-t2-008): Enforce cost and runtime budgets in the Quarry executor, meter browser time, and clamp crawls to the remaining entitlement
Reference-side check: **partial** (confidence high).

**Corrections:** 1) Firecrawl's browser-session create does NOT hold/reserve the estimated cost for paying teams: it runs an Autumn credit CHECK for the full TTL (402 if short) and reserves only a concurrency slot; a debit-ahead exists only for keyless/free usage (updateKeylessBrowserCredits). Rephrase 'reserve then settle and release the remainder' as 'admission check at full TTL, settle actual duration exactly once under a row lock, refund keyless pre-debits'. 2) CostTracking's throwing limit exists as a class feature, but no OSS caller sets a limit (all 'new CostTracking()'); do not present it as an enforced per-request budget in Firecrawl. 3) At crawl submit Firecrawl stores only the clamped limit; the requested limit is logged, not stored on the job. 4) Apify: when max total charge is reached the platform aborts the run automatically and charge()/pushData stop; there is no documented 'finish current step, persist partial results, budget-exhausted status' - that is a Verevon design proposal (INF). 5) Firecrawl's rate classes are plain browser vs session-that-used-a-prompt (whole session re-rated) plus a ZDR surcharge; 'human vs AI-driven' is an interpretation.

**Caveats:** Apify half is DOC-only (pay-per-event SDK, run cost display); not observed in code; Firecrawl CostTracking limit is dormant in OSS (never set); enforcement may exist only in hosted services (INF); Firecrawl 420 credits/hour interact rate is in code only; public pricing shows 2 credits/min; Browser sessions require HANGAR_URL (hosted browser service); self-hosted Firecrawl lacks them

### [F-t2-009](#F-t2-009): Route PDFs, Office files and images to the right parser in /v1/scrape (Content-Type plus magic bytes)
Reference-side check: **partial** (confidence high).

**Corrections:** The OOXML/ODF check sniffs only the PK 03 04 local-file header and excludes genuine ZIP media types. It does not look for [Content_Types].xml. The first-512-bytes window is a recommendation: the reference computes HANDOFF_SNIFF_BYTES from its signature table. The order is: a GCS PDF reference first, then the Content-Type header, then magic bytes. Image routing is not always on: it needs a deployment switch (IMAGE_OCR_ENABLED plus a FirePDF base URL) and the per-request parsers option. Otherwise images are rejected as unsupported files. Archives, audio, video and executables are rejected explicitly. The rest of the mechanism, including the prefetch handoff so parsers do not re-download, is accurate.

**Caveats:** SRC (Firecrawl repo); Image OCR handoff is deployment-gated and option-gated; PDF parsing quality depends on the FirePDF service, which is external in the hosted product; Apify Website Content Crawler selects files by content type but only downloads them and does not parse them (DOC), so '1 of 4' holds for parser routing

### [F-t2-010](#F-t2-010): Turn on the LLM browser planner by default, with budgets, a browsing procedure prompt and connector-first routing
Reference-side check: **partial** (confidence high).

**Corrections:** 1) The Firecrawl loop description is overstated. The single tool accepts free-form agent-browser CLI strings, && chains included, executed through the sandbox's bash. The only command guardrail is a regex denylist for 'tab new'/'open', so 'guardrails reject commands outside the allowed set' is wrong (there is no allowlist). There is no snapshot diff: the model re-runs snapshot and gets the full output. The only stops are the 25-step cap and a per-command timeout, with no token or cost budget. Per-run step/time/token budgets and an allowlisted verb set are Verevon additions, not Firecrawl practice. Because its guardrails are weaker than Verevon's 'one governed action per call', Firecrawl should be cited for loop shape only (capped interactive snapshot, refs, step cap, action-log reminder), not for guardrails. 2) The ladder 'AX ref -> semantic locator -> set-of-marks -> coordinates' is a Verevon synthesis. Aside's actual ladder is interactive snapshot -> full snapshot -> wait and re-snapshot -> annotated screenshot (ref boxes) or raw screenshot, plus 'unconfirmed until re-snapshot' and 'switch strategy after 2-3 failures'. Attribute it accordingly. 3) Codex connector-first routing i…

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Codex procedure, efficiency caps and connector-first rules are DOC (shipped skill/doc text), enforced only by model compliance; Aside prompt is SRC from the daemon SEA blob; provider-specific overrides exist and which harness (v1/v2) a surface uses is INF; Firecrawl interact runs inside the Hangar sandbox (hosted); ZDR switches to a different model with store:false (per record, not re-read)

### [F-t2-011](#F-t2-011): Let users browse freely: a human-session policy plus an omnibox that routes free text to Verevon search
Reference-side check: **confirmed** (confidence high).

**Corrections:** None to the mechanism. Wording: 'free cross-site browsing' for Codex is inferred from the absence of a URL allowlist for human navigation (INF). The 'human-session policy' framing is a Verevon proposal. Routing non-URL text to Verevon search is the recommendation; which search provider Codex's suggestions use was not verified.

**Caveats:** Codex browser pane, extensions and browser-use are remote config requirements that default to false (flag or plan gated); the in-app browser is not universally available; Codex autocomplete runs through session.autocomplete.createController, which stock Electron does not offer: INF that this is a patched Electron/Chromium omnibox; suggestion sources unverified; Aside AI-mode omnibox members are upstream Chromium composebox/AIM names wrapped in an Aside private extension API; whether Google AI Mode is reachable is unverified; REA not used; evidence from grep over pre-extracted bundles and installed files

### [F-t2-012](#F-t2-012): Chat beside the browser: a dockable browser panel bound to the chat run
Reference-side check: **confirmed** (confidence high).

**Corrections:** Minor: Aside injects the sidecar framing (sidepanelPrompt) only on the first user message, for the first attached tab, not on every first mention of a tab. Codex destinations are new-window, left, right, bottom, pin, chat and new-chat, plus the hidden-browser-use host, so 'detached' corresponds to new-window. That the floating composer 'posts into the active chat' is inferred from the thread-scoped composer settings; the send path was not traced.

**Caveats:** Codex browser pane sits behind the server-delivered gate browserPane (client default false), so it may be rollout-gated; Aside evidence comes from the installed extension and the daemon SEA blob (SRC of a shipped artifact); Aside is a local single-user browser

### [F-t2-013](#F-t2-013): Co-browsing: the agent works in the user's visible session in agent-flagged tabs, with takeover enforced inside Quarry
Reference-side check: **partial** (confidence high).

**Corrections:** Codex has no general 'user took over' state that cancels in-flight steps: user_took_over is only the decline reason of the browserAuth secure sign-in elicitation, and in the IAB the user can interact with the page at any time with no input lock or control arbitration found. requestManualHandoff is supported only on the cloud CDP backend. The fail-closed mention resolution (match providerTabId + title + url, never guess ids) is written as model instructions in plugin docs, not code-enforced; claimTab is for the external-browser (extension/cloud) backends, while IAB mentions resolve via tabs.get. Aside shows a 'controlling the tab / Take over' strip (pak strings, DLL symbol) but no daemon-side takeover handling was found (absence claim, INF). So 'takeover enforced inside Quarry' via control_mode + USER_TOOK_OVER per step is a Verevon design with no reference implementation observed; say so. Confirmed: Codex markTab handoff|deliverable with turn-scoped release, Interrupt/SubagentStop/Stop hooks ending the turn, hidden agent tabs; Aside owned vs borrowed session_tabs with idle sweep and orphan reconciliation.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Codex fail-closed tab claiming is DOC (prompt instructions to the model), not runtime enforcement; Codex manual handoff is cloud-backend only; Aside takeover is UI-string/symbol evidence only; enforcement mechanism unknown; Firecrawl browser sessions expose an interactiveLiveViewUrl (human can interact with the agent's session), so a co-browsing-adjacent capability exists in a third source; the '3 sources' denominator is unclear; No source observed implements server-side control-mode arbitration of agent steps

### [F-t2-014](#F-t2-014): Surface one canonical agent run trace in the UI: typed events, receipts, live view, correlation envelope
Reference-side check: **partial** (confidence high).

**Corrections:** 1) Firecrawl run trace: only the API contract (openapi plus types) and a proxy controller are in the repo. The events come from an upstream agent-beta service (EXTRACT_V3_BETA_URL), so the producer, persistence and ordering are not inspectable, and the feature is beta-gated. producerSequence is monotonic per emitting agent, not global: consumers group by agent.id and then order by sequence. Mark it 'contract observed, implementation upstream'. 2) Firecrawl x-request-id has no edge middleware. It is handled per route: provider tools adopt or generate and echo it and use it for safe retries, research/exchange/support proxies forward it, and Exchange billing is keyed by it. Request-row attribution uses a separate External-Request-Id header. Reword 'caller's x-request-id is logged, forwarded upstream and reused as the idempotency key' to cover only those routes. The proposed tower-layer design is Verevon's own. 3) Run-trace prevalence '2 of 4' is generous: Codex has size-capped WebMCP activity, response meta and drained notifications, but no ordered, versioned run envelope. Say 1 of 4 full envelope (Firecrawl, beta) plus Codex for UI-side pieces. 4) Codex details: the open-tab list wi…

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Firecrawl agent trace is beta-only (EXTRACT_V3_BETA_URL) with an upstream closed implementation; Apify meta.origin is DOC (verified on the public page); the 10M-char log cap and secret redaction are DOC from other Apify pages and were not re-verified here; Codex behaviour comes from a prettified copy of the shipped browser-service bundle (SRC); the notification gate depends on credential-observation state

### [F-t2-015](#F-t2-015): Actionability checks and trusted input for every ref-targeted action, then rich input actions
Reference-side check: **partial** (confidence high).

**Corrections:** Mechanism must not say Codex uses DOM.getNodeForLocation: Codex hit-tests with an in-page elementFromPoint probe (shadow-root aware) injected via DOM.resolveNode + Runtime.callFunctionOn per frame hop, using getContentQuads/getBoxModel for coordinate transforms. Order is: hit-test first, DOM.scrollIntoViewIfNeeded only if not hittable, then poll every 25 ms (500 ms deadline, 0.5 px tolerance) for two consecutive matching samples (timer-based, not animation frames). Errors: 'covered by <tag> or otherwise not hittable', 'did not settle after scrolling', and 'couldn't be clicked' for unpositioned nodes. 'Rich input 1 of 3 (unique)' overstates: Aside also has trusted CDP input with modifiers, insertText, file-chooser interception and download events; only the virtual clipboard shim and held clicks/drag with xdotool key names are Codex-specific. Aside falls back to DOM activation (with checked-state verification) when the target is obscured rather than simply erroring.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Actionability is also built into Playwright (auto-wait), which Apify/Crawlee and Firecrawl's playwright-service inherit; the 2-of-3 count reflects custom implementations only (INF); inputTargetToken semantics in Codex key synthesis are inferred from naming (INF); not a public CDP parameter; Codex AX action set (held clicks, drag, paste formats) is DOC-backed (Codex plugin docs/api.json); clipboard bridge is SRC; Aside evidence comes from the daemon Node SEA blob (SRC), not a documented API

### [F-t2-016](#F-t2-016): Scoped, robots-compliant crawl: wire CrawlFrontier scoping, robots enforcement and typed denials into the live crawl path
Reference-side check: **partial** (confidence high).

**Corrections:** 1) The Rust filter_links returns only typed codes; the remediation sentences are built in TypeScript (WebCrawler.filterLinks / DenialReason enum), so split 'native Rust filter_links with remediation messages'. 2) Several remediation texts still name v1 options (maxDepth, allowBackwardCrawling) and the ROBOTS_TXT text tells callers to set ignoreRobotsTxt, which is enterprise-gated; no message names allowSubdomains (external-link denials name allowExternalLinks). 3) Check order differs slightly: depth and file type come before fragment/scope, and robots is checked before social media. 4) Pattern caps: 1000 patterns total across include+exclude (not per field) and 100k characters total.

**Caveats:** Apify enqueueLinks/transformRequestFunction and respectRobotsTxtFile are DOC-only, and robots is opt-in in Crawlee (not on by default); Firecrawl robots enforcement is on by default for crawl only; scrape is opt-in via team flag or Safe Mode; overrides are enterprise flags; Firecrawl fails open when the robots.txt fetch fails and deliberately ignores Crawl-delay

### [F-t2-017](#F-t2-017): Public async job lifecycle: status, paginated results, per-URL errors, cancel/pause/resume, origin and live tail
Reference-side check: **partial** (confidence high).

**Corrections:** Pause/resume in the title is not a reference capability. Firecrawl crawls have only cancel (pause/resume exists only for Firecrawl monitors), and Apify has abort, graceful abort and resurrect of a finished run, but no pause. Say that pause/resume comes from Quarry's own Temporal signals, or that it is a Verevon addition. Last-Event-ID resume and an event-history tail are recommendations: neither reference has them. The Firecrawl WS has no resume cursor and polls once a second, and the source has no SSE. Apify's 30 s grace period comes from the runs page; the system-events page gives no duration. The 'recorded steps' timeline comes from Firecrawl's legacy multi-page extract (extract-redis), not from crawl jobs.

**Caveats:** Apify half is DOC-only (public docs, not code); Firecrawl half is SRC and was verified in the local repo; Firecrawl stores a per-team crawlTtlHours override in account flags; the default is 24 h; Firecrawl only registers a WS route for /crawl/:jobId; a batch WS variant is SDK-side and unverified on the server; Firecrawl step timeline is legacy extract only (medium confidence)

### [F-t2-018](#F-t2-018): Make scrape, crawl and batch schedules actually execute (Temporal schedules)
Reference-side check: **partial** (confidence high).

**Corrections:** 'FOR UPDATE SKIP LOCKED' is not observable in Firecrawl: the claim is the RPC monitoring_claim_due_monitors whose SQL is not in the open repo, so present SKIP LOCKED as the Verevon design choice (INF). Firecrawl's scheduler drives change-tracking monitors (scrape/crawl/search targets), not generic scheduled scrape/crawl/batch jobs. Timezone-aware cron exists in both Firecrawl (cron.ts) and Apify, not Apify-only. Apify's default failure email covers a scheduled run that fails to start due to misconfiguration, not every run failure; Firecrawl notifications are monitor result summaries (email/Slack/in-app). The credit check happens at run time via a credit lock in the runner (check recorded as skipped_no_credits), not before dispatch in the scheduler tick.

**Caveats:** Apify side is DOC-only; Firecrawl monitoring is recent (2026) and partly routed through a billing rollout (firebill) for some orgs; behaviour may change; Firecrawl monitors require the DB-backed (cloud-style) deployment; self-host availability not verified

### [F-t2-019](#F-t2-019): Structured extraction that works in production: json, summary, question and highlights formats over a real Model Plane structured-output grant
Reference-side check: **partial** (confidence high).

**Corrections:** 1) normalizeSchema does not resolve $ref; it normalises $defs in place and routes schemas with $ref/$defs to a larger model. 2) Fallback is to a second model (gpt-4.1-mini, same OpenAI provider) only on quota/rate-limit errors, not 'next provider on provider error'. 3) The 10,000-char figure is the max length of the question/query/prompt text, not the markdown input; question/highlights trim the page by tokens to the model context and summary uses trimToTokenLimit. 4) question/highlights run on a Firecrawl-hosted fine-tuned model (Fireworks) or Gemini through query.ts, not the json-extraction routine. 5) Prevalence: Apify Website Content Crawler offers an optional per-page AI summary (apify-platform-049, DOC), so 'summary' is at least 2 of 4; json/question/highlights as built-in scrape formats remain Firecrawl-only in this record set.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Firecrawl models are hard-coded to OpenAI families plus a private Fireworks fine-tune and Gemini; Verevon must route through Model Plane; question/highlights depend on a hosted fine-tuned model id; self-hosters may not have it (INF from model id string); /extract is deprecated in Firecrawl (Deprecation headers), consistent with the item

### [F-t2-020](#F-t2-020): Harden persistent browser profiles: complete snapshots, per-org envelope encryption at rest, ordered saves, ZDR enforced inside Quarry
Reference-side check: **partial** (confidence high).

**Corrections:** 1) The snapshot spec (all cookies with expiry, SameSite and partition key, per-origin storage, IndexedDB export) and 'encrypted with a per-org key' must not be attributed to Firecrawl. Firecrawl keeps only a profile listing; the profile state lives in the external Hangar service, and no encryption code exists in the repo. These are Verevon requirements, from Verevon's own docs/CONTRACTS.md:185-199. 2) Apify's encryption (AES-256-GCM key wrapped by a per-(user, Actor) RSA key) applies to secret Actor input fields, not to browser profiles or session cookies. No reference shows envelope encryption of browser profiles, so present it as a security baseline borrowed from the input-secret pattern, not parity. 3) The Firecrawl tombstone is a key-value entry keyed by team hash and name, not a deleted_at column; the save-ordering mechanism is otherwise accurate. 4) Caller cookies are seeded with Playwright context.addCookies using a 'last two host labels' heuristic, which is not a registrable/public-suffix domain and is wrong for co.uk-style suffixes. It is not CDP Network.setCookies. Fix the wording. 5) Prevalence '2 of 4 (apify, firecrawl)' undercounts persistence: Codex keeps browser sto…

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Firecrawl profile contents and at-rest handling live in Hangar (closed and hosted); only the listing and ordering logic is SRC; Apify evidence is DOC only; Chromium-based agent browsers (Codex, Aside) likely protect cookies at rest with OS-bound keys (INF, not verified here); this is local-device encryption, not per-org; The per-org envelope encryption design is Verevon-internal intent (DOC CONTRACTS.md), not a reference feature

### [F-t2-021](#F-t2-021): Secret redaction at capture time: AX/DOM snapshots, screenshots, OCR, receipts and logs, before storage or model exposure
Reference-side check: **partial** (confidence high).

**Corrections:** (1) Scope is overstated. No reference redacts screenshots, OCR output or receipts. Aside's rules apply to text and ARIA snapshots for durable Context Awareness capture, plus redaction of tool arguments and error bodies in logs. (2) 'Before model exposure' is only partly true. The interactive agent snapshot path redacts only non-empty password inputs, so other secrets on a page reach the model. (3) The fill mechanism is wrong in one detail and over-generalized. Aside's origin-checked fillPasswordInput sets the value with the native HTMLInputElement value setter plus input/change events, not Input.insertText. Its only call site is autofillUnlockPassword, which unlocks an external password-manager extension page (chrome-extension:// origin), not a general website login fill. Aside Vault website fills go through the Vault extension with agent-session, policy and URL-match checks (record 054). (4) Prevalence: Firecrawl also has opt-in output redaction (redactPII on markdown through an external fire-privacy service, failing closed), and Apify's redaction covers only secret env vars in run logs. Restate this as: capture-time page redaction is 1 of 4 (Aside); some redaction is 3 of 4 (Asi…

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Aside evidence is a bundled string or byte offset in the scratch copy of the NODE_SEA_BLOB; no runtime confirmation; The Context Awareness passive capture is the durable path; whether it is on by default or opt-in was not determined; Apify secret log redaction is DOC-only; Firecrawl redactPII depends on an external fire-privacy service that is not in the repo

### [F-t2-022](#F-t2-022): Org-pinned, tighten-only web policy: tri-state capability gates delivered as signed claims and enforced by Quarry
Reference-side check: **partial** (confidence high).

**Corrections:** Firecrawl Safe Mode is not strictly tighten-only: an org admin can set allowBypassSafeMode so a request may send safeMode:false, and allowlisted domains switch off robots, stealth, authentication and site-handling controls. 'All violations produce audit events' is overstated: rejected-scrape SIEM events are emitted only from the scrape, crawl, batch-scrape, extract and agent controllers, and only when the team has the siemLogging flag and SIEM is enabled for the org. Worker-side downgrades are silent, and the map and search rejections emit nothing. 'Signed claims' is Verevon's own design; Firecrawl reads flags from the cached account chunk (ACUC). Codex browser gates are vendor rollout feature flags (client defaults are false until server values load; the env override works only in Dev builds), not org-admin policy. The exception is the enterprise requirement browser_use_full_cdp_access. maxBrowserSessions is declared in TeamFlags, but no enforcement was found in the repo.

**Caveats:** Safe Mode and tri-state flags are enterprise or support-enabled team flags ('contact support'), SRC; Safe Mode has a per-request bypass (if the admin allows it) and a domain allowlist, so admin config can widen it; SIEM audit events are emitted only when the siemLogging flag is on; Codex gates: only client-side defaults observed; actual server-delivered values not observed

### [F-t2-023](#F-t2-023): Scrape-time page actions plus screenshot and PDF formats, under one bounded, grant-gated action policy
Reference-side check: **partial** (confidence high).

**Corrections:** Firecrawl action execution happens only in the closed hosted fire-engine (chrome-cdp); the open-source playwright engine declares actions:false, so 'execute sequentially on one page' is not observable in source and self-hosted Firecrawl cannot run actions. Outputs are grouped into per-type arrays (screenshots, scrapes, javascriptReturns, pdfs), not one ordered per-action results array (an ordered array is a Verevon improvement, mark INF). The scrape executeJavascript action has no script-size cap; the 100,000-byte / 1-300 s limits belong to the separate hosted-browser /execute endpoint (Hangar), so do not attribute a script cap to the scrape action DSL. Aside always scroll-stitches with captureBeyondViewport:false (single tile when content fits the viewport); the 'captureBeyondViewport when within limit' branch is Codex/Firecrawl-style, so the merged mechanism blends sources and should say so. JPEG q80 and fresh-frame screencast are Codex-specific; Aside tiles are PNG and element shots default to webp.

**Caveats:** Firecrawl actions: SRC for schema/limits, but runtime behaviour lives in closed fire-engine (cloud-only); Apify page actions are DOC-only and are declarative per-Actor options (Website Content Crawler, RAG Web Browser), not a general action DSL; Screenshot prevalence may be undercounted: Apify/Crawlee likely also offers screenshots (INF, not re-checked); Firecrawl OpenAPI spec lists eight action types (no pdf): spec drift

### [F-t2-024](#F-t2-024): Credential handoff where the model never sees secrets: executor-side fill_secret and email OTP via integration-corev2
Reference-side check: **partial** (confidence medium).

**Corrections:** 1) Re-attribute: Codex browserAuth (secure sign-in handoff, isolated-world native delivery, credential observation gate, email OTP) is wired to the CDP client in the hosted 'gaas-browser-environment' (Codex Cloud Browser) via a broker socket; the shipped desktop in-app browser (iab client) does not advertise the browserAuth capability and the broker socket env is forwarded only in Dev builds. Say 'Codex Cloud Browser', not 'Codex in-app browser'. 2) Email OTP: the broker challenge reports email_otp_delivery and native delivery; 'the host fetches the code from the mail connector' is inferred from the elicitation text, not from observed connector code - mark INF. 3) Codex 'vault/saved-login picker' rests on UI strings (string-level evidence); the opaque-ref vault fill without a UI form is Aside's (SRC). 4) Prevalence '2 of 3, table-stakes' overstates: Aside (opaque password refs, SRC) and Codex (cloud-only) - call it 2 of 4 sources, differentiator, with Codex's variant cloud-only.

**Prevalence in the first draft was wrong; the corrected figure is given in the card.**

**Caveats:** Codex browserAuth is cloud-browser (gaas) only and depends on an OpenAI-hosted broker; not available in the local desktop in-app browser per the shipped client capability list; Elicitation is not answerable by an automated guardian (codex_guardian_compatible:false) - SRC; Aside returns generated-password plaintext only to its trusted agent-manager extension (not the model) - SRC; browser_auth telemetry/gate names suggest server-side gating; exact flags not confirmed (INF)

