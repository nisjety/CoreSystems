# T2 final: Quarry v2 and the Verevon in-app browser compared with Firecrawl, Apify, Codex and Aside

This was a read-only audit. Code is treated as ground truth; repo docs show intent and are often stale.

**Provenance tags:**
- **REA**: reverse-engineered from a shipped app
- **SRC**: read in source
- **DOC**: documentation only
- **INF**: my inference

**Limits on the evidence:**
- All Apify evidence is DOC.
- Firecrawl's hosted parts (fire-engine, Hangar) are seen only through its open-source client contracts.
- Codex and Aside evidence comes from static analysis of the installed apps (REA/SRC).
- Nothing in Verevon was run. Every "fails in production" claim is INF.

## 1. Calibration

| Tier | Count | Basis |
|---|---|---|
| Priority | 24 (F-t2-001 to 024) | Blocks the shared user+AI browser or the scrape API, a table-stakes gap, or a safety risk |
| Recommended | 41 (F-t2-025 to 065) | Clear value, or large work that needs sequencing |
| Nice-to-have | 12 (F-t2-066 to 077) | Polish and speculative bets |
| Dropped | 2 | R-t2-02-11 (design extraction is niche); R-t2-10-05 (a Slack entry point would pre-empt the Channel Plane, which is future work) |

The category consolidators had put 51 items in Priority. I cut that to 24 by merging duplicates and demoting items that do not block the vision.

**Main merges:**
- **Production browser grants** (raised in three categories): F-t2-001
- **Scrape options and formats contract:** F-t2-005
- **Structured extraction:** F-t2-019
- **Crawl scoping and robots.txt** (raised in three categories): F-t2-016
- **Job status and cancel API:** F-t2-017
- **Budget enforcement** (raised in three categories): F-t2-008
- **Browser profile encryption:** F-t2-020
- **Secret redaction:** F-t2-021
- **Org policy:** F-t2-022
- **Co-browsing:** F-t2-013
- **Credential handoff:** F-t2-024
- **Webhooks:** F-t2-031
- **Per-site consent:** F-t2-032
- **Agent observation quality:** F-t2-034
- **External contract and MCP:** F-t2-064

**Demoted to Recommended:**
- Per-org capacity limits: F-t2-025
- Browser command deadlines and honest bot-block handling (both still early quick wins): F-t2-026, F-t2-027
- Page context to chat (Verevon status unknown): F-t2-028
- Metrics: F-t2-029
- Monitor diff quality: F-t2-030
- PDF/OCR depth: F-t2-037. The small file-routing fix stays in Priority as F-t2-009.

**Rescoped.** R-t2-03-11 wanted to switch on Quarry's stealth scripts. That contradicts R-t2-04-03 (delete the stealth code and report blocks honestly) and Verevon's no-bypass rule for bot challenges. It is now F-t2-066, a nice-to-have limited to cookie-consent banners and ad filtering. Fingerprint masking waits for an ADR.

**Two code spot checks I ran (SRC, repo commit afb8392):**
1. **Human takeover is not enforced in Quarry.** The gateway checks it (`browser.rs:705-723` and `:2071-2090`), but Quarry's `/step` path in `agent_routes.rs` never reads `control_mode`. A Model Plane run that calls Quarry directly keeps acting while the user has taken over. This corrects the input strength C-t2-08-017. Phase 1 of F-t2-013 fixes it in the first wave.
2. **The gateway search cache is confirmed unscoped.** The cache key in `search.rs:218-226` has no org or user, and results are stored without any ZDR check. They are kept for 4 hours and served stale when Quarry errors. Whether cached results can include org-private corpus hits is INF.

**Verification.** 32 of the 77 items still have `needsVerification` set. The key ones:
- F-t2-001: the 401/403 failures on the AI paths are INF.
- F-t2-007: how often the viewport-triggered browser relaunch happens.
- F-t2-010, F-t2-014, F-t2-020, F-t2-024, F-t2-025, F-t2-028, F-t2-045.
- F-t2-021: whether Chromium already masks password values in the accessibility tree.

## 2. Waves, quick wins, ADRs, existing plans

**W1 – unblock and make safe:**
- F-t2-001: production browser grants
- F-t2-002: gate script evaluation
- F-t2-003: search cache ZDR and tenant scoping
- F-t2-004: ZDR-safe telemetry
- F-t2-005: inline scrape options contract
- F-t2-006: reap abandoned runs
- F-t2-007: browser pool without relaunches
- F-t2-009: file routing
- F-t2-026: browser command deadlines
- F-t2-027: honest bot-block handling
- Phase 1 of F-t2-013: takeover enforced in Quarry

**W2 – table-stakes parity:**
- Browser: F-t2-010 to 015 (planner on, free browsing, chat beside the browser, co-browsing, run trace, trusted input)
- Acquisition: F-t2-016 to 019 and F-t2-023 (scoped crawl, job lifecycle, schedules, extraction, scrape actions and screenshots)
- Guardrails: F-t2-008, 020, 021, 022, 025, 029, 045

**W3 – governed depth:** F-t2-024, 028, 030 to 042, 044, 046 to 050.

**W4 – platform and leverage:** F-t2-043, 051 to 065.

**W5 – nice-to-have:** F-t2-066 to 077.

F-t2-001 and F-t2-005 are the two foundations. F-t2-022 needs its ADR before W2, because F-t2-008, 025, 032 and 058 read their caps from it. F-t2-024 is Priority but lands in W3 because it depends on F-t2-001, 013, 015 and 021.

**Quick wins** (effort S or M, significance 4 or more, no unbuilt cross-plane dependency): F-t2-002, 003, 004, 005, 006, 007, 009, 026, 027 and 045, plus phase 1 of F-t2-013.

**ADRs needed before building:**
1. Who owns web/browser policy (Control Plane org-core or Model Plane capability-core), with tighten-only signed claims: F-t2-022, 032, 008, 025.
2. Who holds secrets used inside browser pages, across planes: F-t2-024, 021.
3. How a grant binds shared human+agent sessions, plus the control lease and the human navigation policy: F-t2-001, 011, 013, 054.
4. What ZDR means for each feature (refused, inline-only or hash-only): F-t2-003, 004, 016, 018, 030, 048.
5. Anti-bot posture: F-t2-027, 066, 067, 016, 058.
6. Script execution model (evaluate, code mode, generated extractors): F-t2-002, 023, 063, 065.
7. Exposing Quarry and connectors over MCP, which reverses the documented deferral: F-t2-064, 042.
8. Run trace envelope and sharing runs into Spaces: F-t2-014, 038, 072.
9. Which plane owns follow-up automation and agent routines: F-t2-031, 061.
10. Browser substrate (shared pool, per-session VM, remote providers, user-browser extension): F-t2-007, 059, 060, 077.

**Plans already in Verevon docs, and whether they match the best reference practice:**

| Item | Plan (DOC) | Match? |
|---|---|---|
| F-t2-001 grants | Gateway comment says unbuilt; `BROWSER_WORKSPACE_PLAN` says COMPLETE | The doc is stale; the intended design is right |
| F-t2-005 formats | ROADMAP Phase 6; `OSS_PARITY_BACKLOG:22` claims done | The "done" claim is false; the plan misses inline return as the ZDR path |
| F-t2-008 budgets | `WEB_AGENT_ROADMAP:50-53` says enforced | Contradicted by the code |
| F-t2-016 crawl, F-t2-041 discovery | ROADMAP Phase 5 | Matches Firecrawl; needs an audited robots override and fix-it denial messages |
| F-t2-017 job lifecycle, F-t2-018 schedules | ROADMAP Phase 5 (Temporal) | Matches; add a "skipped, no credits" state |
| F-t2-019 extraction | Model Plane grant deferred, no owner | Incomplete; the format work is not planned |
| F-t2-020 profiles | CONTRACTS and SOVEREIGN Phase 3 | Encryption matches; snapshot completeness and tombstones are missing |
| F-t2-023 scrape actions | SOVEREIGN Phase 6 | Matches; add caps and the evaluate gate |
| F-t2-024 credentials | Plan section 19.1 (`fill_secret`) | Codex shape; missing email OTP and isolated-world fill |
| F-t2-027 bot-block | Challenge classifier planned | Matches; remove stealth code |
| F-t2-066 identity | Backlog: fingerprint rotation on block | Diverges from the honest posture; needs an ADR |
| F-t2-037 documents | PARITY plan says DOCX shipped | Stale: DOCX has no caller |
| F-t2-057, F-t2-058, F-t2-059 | SOVEREIGN Phases 2, 4, 5 and the remote-provider SSRF gate | Match, some stronger than the references |
| F-t2-031 webhooks | ROADMAP Phase 9 | Misses tenant scoping and SSRF protection |
| F-t2-064 MCP | Deferred | Diverges: Apify and Firecrawl both ship MCP |

## 3. What every reference shares

There are 61 table-stakes capabilities. They are judged within the scraping pair (Firecrawl, Apify) or the browser pair (Codex, Aside), or as present in 3 of the 4.

**Verevon already ships (keep):**
- Web search (Verevon is ahead)
- The multi-tier fetch engine
- Proxy plumbing
- Polite per-host throttling
- The durable URL queue
- Browser profiles with persistent logins
- The agent-control indicator and live view
- Accessibility-tree observation with stable references
- Stale-reference protection
- Semantic locators across frames
- Dialog handling
- Fail-closed approvals
- Risk-tier confirmations
- Visual proof of agent work
- Takeover, though it is enforced in the gateway, not in Quarry

**Half-done (partial or stub), with the item that closes it:**
- F-t2-005: inline scrape and main-content controls
- F-t2-016: crawl scoping
- F-t2-023: screenshot/PDF output and scrape actions
- F-t2-037: Office conversion
- F-t2-007: browser pool
- F-t2-020: persistent sessions
- F-t2-008: crawl budgets
- F-t2-041: sitemap discovery
- F-t2-018: schedules
- F-t2-047: retries
- F-t2-058: proxy tiers
- F-t2-066: TLS impersonation
- F-t2-046: metering and plans
- F-t2-014: correlation ids
- F-t2-003: run retention and the ZDR record
- F-t2-036: least-privilege run tokens
- F-t2-012: chat beside the browser
- F-t2-011: free navigation
- F-t2-069: page utilities and screenshots
- F-t2-068: history and clear-data
- F-t2-001: authenticated control channel
- F-t2-015: trusted input
- F-t2-034: settle and set-of-marks screenshots
- F-t2-013: tab ownership and takeover
- F-t2-032: permission modes
- F-t2-039: uploads and downloads
- F-t2-049: prompt-injection guard

**Lacking (absent or unknown):**
- F-t2-064: MCP server, CLI and skills
- F-t2-016: robots.txt compliance
- F-t2-013: co-browsing on the user's tab
- F-t2-024: credential handoff
- F-t2-063: code-mode control and its sandbox hardening
- F-t2-032 and F-t2-010: settings hub, capability-aware tool docs and a browsing procedure prompt

**Best of breed:**

| Capability | Best source | Why | Item |
|---|---|---|---|
| Inline scrape and strict options | Firecrawl (SRC) | One shared strict schema reused by every endpoint; formats returned inline. Do not copy its silent TLS-verification-off default | F-t2-005 |
| Job lifecycle | Firecrawl (SRC) | Cursor pages, per-URL errors, cancel, and a "processing continues" timeout contract | F-t2-017 |
| Crawl scoping and robots | Firecrawl (SRC) | Typed denials with fix-it messages for LLM callers; robots on by default, overriding it is an entitlement | F-t2-016 |
| Schedules | Firecrawl (SRC) and Apify (DOC) | Overlap and no-credit skip states, jitter, time zones | F-t2-018 |
| Structured extraction | Firecrawl (SRC) | Strict schema handling, JSON repair, model chosen by schema complexity | F-t2-019 |
| Screenshot and PDF | Aside (SRC) | Tile-and-stitch for tall pages, element shots | F-t2-023 |
| Scrape actions | Firecrawl (SRC) | One atomic, capped action list | F-t2-023 |
| Browser pool | Apify (DOC) | Max open pages and retire-after-N | F-t2-007 |
| Profiles | Firecrawl (SRC) | No profiles under ZDR, monotonic saves, tombstones | F-t2-020 |
| Executor budgets | Apify (DOC) and Firecrawl (SRC) | Charge within the cap, or reserve then settle | F-t2-008 |
| Proxy escalation | Firecrawl (SRC) | Escalates only on 401/403/429 | F-t2-058 |
| Control channel | Aside (SRC) | Signed-challenge browser-control tokens | F-t2-001 |
| Trusted input | Codex (SRC) | Hit-tests through cross-process iframes; typed "covered by" errors | F-t2-015 |
| Settle after actions | Aside and Codex (SRC) | Quiet window plus first paint | F-t2-034 |
| Co-browsing | Codex (SRC) | Agent tabs the user can reveal; handoff marks | F-t2-013 |
| Chat beside the browser | Codex (REA/SRC) | Docked browser with a floating composer | F-t2-012 |
| Credential handoff | Codex (SRC) | Isolated fill, email OTP, observation blocked after the fill | F-t2-024 |
| Permissions and consent | Codex (SRC) | One policy choke point, typed denials, per-site consent where deny wins | F-t2-032 |
| Uploads and downloads | Codex (SRC) | One-time grants | F-t2-039 |
| Run history and ZDR records | Firecrawl (SRC) | ZDR-scrubbed records | F-t2-003 |
| Run trace | Firecrawl and Codex (SRC) | Versioned event envelope | F-t2-014 |
| MCP for external agents | Apify (DOC) | Hosted MCP with OAuth | F-t2-064 |
| Code mode | Aside and Codex (SRC) | One script tool over a typed browser runtime | F-t2-063 |
| Metering | Firecrawl (SRC) | One pricing function; failed requests are free | F-t2-046 |

## 4. What each reference does uniquely well

**Firecrawl (SRC; hosted parts seen only through contracts).** Worth taking:
- Strict shared options and a formats registry: F-t2-005
- Safe Mode, an org policy that requests cannot widen: F-t2-022
- Rules for what must never be cached, plus a freshness setting: F-t2-003, 048
- A tracing sampler that drops ZDR traffic: F-t2-004
- An engine capability matrix with typed re-planning: F-t2-057
- Fair per-tenant backlog: F-t2-025
- URL de-duplication: F-t2-041
- Monitors with normalised diffs and an AI judge: F-t2-030
- A PDF pipeline that runs OCR only when needed: F-t2-037
- Cached generated extractors (Verevon should generate its own declarative extraction profiles, not code): F-t2-065
- A research job API: F-t2-062
- Exporting a successful run as a reusable skill: F-t2-051
- A URL reputation gate: F-t2-049
- A local test site with prompt-injection pages: F-t2-044
- An API deprecation registry: F-t2-075
- Provider tools discovered by the page's domain: F-t2-043

Its webhook delivery is weaker than Verevon's.

**Apify (DOC only).** Worth taking:
- Packaged, reusable job units and saved tasks: F-t2-053
- Per-run result stores with server-side export: F-t2-052
- Charge-within-cap budgets: F-t2-008
- Autoscaling and browser pool lifecycle: F-t2-007, 029
- Learned per-site rendering mode: F-t2-057
- Proxy session health and routing knobs: F-t2-058
- Secret inputs encrypted per user and tool: F-t2-020
- Per-run isolated containers: F-t2-060
- Alerts on data quality: F-t2-029
- Hosted MCP: F-t2-064

Skip the marketplace.

**Codex in-app browser (REA/SRC).** Worth taking:
- Co-browsing in agent tabs, with handoff marks: F-t2-013, 054
- An agent cursor that the click waits for: F-t2-038
- One policy choke point with typed denials and per-site consent: F-t2-032
- Gates on raw script and devtools-protocol access: F-t2-002
- Hit-tested trusted input: F-t2-015
- Settle logic and a frame-aware page snapshot: F-t2-034
- Honest bot-block handling: F-t2-027
- Per-command browser deadlines: F-t2-026
- Connector-first routing before opening a browser: F-t2-010
- The OTP and secret bridge from connectors into pages: F-t2-024
- A memory pollution guard: F-t2-045
- Logs split into safe and sensitive fields: F-t2-004
- One audit record per permission decision: F-t2-036
- On-page annotation: F-t2-028, 038
- Multiple browser backends behind one interface: F-t2-059
- The same browser runtime for evals and production: F-t2-044
- A reviewer model for approvals: F-t2-049

Its embedded browser shell does not fit Verevon's remote, leased Chromium.

**Aside (REA/SRC).** Worth taking:
- The side-panel chat: F-t2-012
- The signed control channel: F-t2-001
- Settle with notes on page events: F-t2-034
- Steering a run and queueing follow-ups: F-t2-038
- Scheduled, heartbeat and event-triggered agent routines: F-t2-061
- Session caps and idle reaping: F-t2-006
- Site playbooks auto-injected by URL: F-t2-043
- An origin-bound credential vault: F-t2-024
- Two-tier, capture-time secret redaction: F-t2-021
- Screenshot tiling: F-t2-023
- Attention notifications that skip sessions you are already watching: F-t2-038
- Background tab hygiene: F-t2-007

Skip its password-manager breadth, user page scripts and ambient activity capture.

## 5. Strengths to protect

- **Search.** The multi-provider router with ZDR gating beats every reference. Only the gateway cache in front of it needs fixing (F-t2-003).
- **SSRF and egress containment.** DNS is resolved once and pinned, redirects are checked per hop, each session gets a pinned proxy, the network guard denies by default and QUIC is off. This is best in class. Extend it to webhooks (F-t2-031) and require it of remote browser providers (F-t2-059).
- **Governed execution.** One action per call; allowed domains come only from the grant, which is rechecked before every action; sensitive grants are one-shot and tightly scoped; dialogs are never auto-answered; risk gates are enforced in code and approvals fail closed.
- **Evidence.** Immutable step receipts, checkpoints, proof bundles, a timeline with replay, and procedure compile and replay checks. Verevon is ahead here; surface it in the UI (F-t2-014) rather than rebuild it.
- **ZDR strictness.** Stricter than any reference's incognito mode. Every new cache, monitor or inline path must keep it.
- **Observation and locators.** Element references bound to native accessibility nodes, fail-closed target repair, cross-process frame actions, and debug collectors that pages cannot tamper with.
- **Platform plumbing.** Per-host politeness, the durable queue with Temporal, webhook delivery with signing, backoff and a dead-letter queue, a machine-readable error envelope, refusal of unsafe production defaults, SCIM provisioning, idempotent provider writes in integration-corev2, hardened MCP client connections, and a first-party OAuth connector broker.
- **The bulk-plus-interactive combination.** Neither Codex nor Aside has a crawl engine. Keep bulk acquisition out of the agent's tab loop and offer it to the Model as jobs it can propose (F-t2-042).

**Correction:** human takeover is enforced by the gateway, not by Quarry, until phase 1 of F-t2-013 lands.

## 6. Themes, risks, open questions

**Themes:**
1. **Built but not wired.** The single biggest return comes from connecting code that already exists: grant issuance, the crawl scoping and robots code, scrape actions, the DOCX parser, the retry helper, the secret redactor, the metrics crates, procedures, the Temporal client and the LLM planner.
2. **Enforcement belongs in Quarry.** Takeover, budgets, org policy and script gating are enforced today in the gateway, in prompts, or not at all.
3. **ZDR as a contract, not a flag.** Inline return as the ZDR scrape path, a cache admission rule, ZDR-safe telemetry, and a published matrix of what each feature does under ZDR.
4. **One contract for the UI, the Model and the public API.** Shared scrape options, grant action classes, typed errors and denials, and OpenAPI generated from the same types.
5. **A browser users and the AI genuinely share.** Free browsing, chat beside the page, co-browsing, visible agent presence, page context and consent.
6. **Custody of secrets and policy.** Tighten-only org policy, capture-time redaction and an executor-side secret bridge.
7. **Docs have drifted from code.** Docs call robots handling closed, format parity done and DOCX shipped, say budgets are enforced, and mark the browser workspace plan COMPLETE.

**Risks:**
- **Blast radius of the grant redesign.** F-t2-001, 011, 013 and 032 all change what a grant means; design them together.
- **ZDR regressions.** Every new cache, monitor, inline payload or telemetry path is a new leak path.
- **Cross-tenant exposure.** The search cache is unscoped, tenant webhooks lack an SSRF guard (INF), and all sessions share one Chromium.
- **Plane-ownership conflicts.** Policy owner, automation owner and run-trace projection must not introduce cross-plane DB access or let Spaces widen access.
- **Compliance.** robots.txt is unenforced on crawl, and the stealth code sits in tension with the no-bypass rule.
- **Unbounded cost.** Budget caps exist in the contract but are not enforced.
- **Evidence captures secrets.** Receipts and frames store secrets until F-t2-021 lands.
- **Evidence quality.** Apify is DOC only, Firecrawl's hosted behaviour is inferred, and nothing was runtime-tested.

**Open questions:**
1. Which plane owns web/browser capability policy, and how are policy claims revoked mid-run?
2. For a human browsing session, should navigation be governed by a broker-defined policy, a re-issued grant, or both? What idle timeout?
3. Can smart-router search results include org-private data? If yes, the gateway cache is a cross-tenant incident.
4. What does ZDR mean for monitors, schedules, durable crawls and URL caches: refused, inline-only or hash-only?
5. Is the honest anti-bot posture official policy? If so, delete `stealth.rs` and the fingerprint-rotation backlog item.
6. Will Verevon expose Quarry and connectors over MCP to third-party agents, and under which OAuth model?
7. Who owns secrets used inside pages, and does every email-OTP use need approval?
8. Do enterprise tenants need a container or VM per browser session?
9. Are code-mode control and generated extraction profiles acceptable if Quarry still validates every call?
10. Which claims need runtime confirmation first: the AI-path 401/403s, the viewport relaunch, password masking in the accessibility tree, and the iframe sandbox flags at `DashboardSearchResults.tsx:184`?
11. Should the stale docs be retired with each wave, and added to `STALE_DOC_DELETION_REGISTER.md`?


<!-- Recovered verbatim from the tiering agent's final message: the harness blocked the agent from writing this .md file itself. -->
