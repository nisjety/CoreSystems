# T2 / Category 4: Anti-bot, proxies & reliability (comparison summary)

Inputs: 51 reference records (apify 12 DOC, aside 2 SRC, codexbrowser 9 SRC, firecrawl 28 SRC) and 18 Verevon baseline records. Result: 25 clusters, 49 records assigned, 2 excluded (Apify proxy price list; Codex MASQUE dev-environment tunnel). Full detail: `04-anti-bot-proxies-reliability.clusters.json`.

## Shared baseline (table-stakes for the scraping half)
Apify and Firecrawl are the two web-scraping benchmarks. Codex and Aside are browsers, so crawler features are judged against those two.
- Proxy pool plumbing (C-001): Verevon is **ahead** (sticky (org,host) hashing, processor approval, per-run affinity, DNS-pinned browser egress).
- Tiered/geo proxies with auto-escalation (C-002): Verevon **partial**. It has a flat proxy list with no tier, no country and no escalation.
- Retries and error hooks (C-011): **partial**. `retry.rs` exists but has no callers.
- robots.txt compliance (C-013): **absent**. It is only applied in `/v1/map`, and docs wrongly say it is closed.
- Politeness throttling (C-014): Verevon is **ahead** (AIMD per-host scheduler plus an autoscaled pool).

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Proxy tiers + auto escalation | Firecrawl (SRC) | `proxy=auto` by default. Escalates to the stronger tier only on 401/403/429 within the retry budget, keeps a flat price, and Safe Mode can downgrade it |
| Proxy routing knobs (group/country/session) | Apify (DOC) | Routing is set through username params, including US-state geo and sticky sessions |
| Session health / retirement | Apify Crawlee (DOC) | Uses markGood/markBad/retire. Rotations don't count against retries, and state persists |
| Honest bot-block handling | Codex (SRC) | Separates real block signals from non-signals, allows one recovery, never changes fingerprint or network, and reports hostname-only telemetry with closed reasons |
| Browser command deadlines | Codex (SRC) | Per-operation timeout table, a 15 s tab-attach handshake, and a single retry when the execution context is destroyed |
| Deadline budgeting | Firecrawl (SRC) | Three abort tiers (external/scrape/engine). Queue waits are capped at 2/3 of the budget |
| Typed errors | Firecrawl (SRC) | About 55 transportable codes plus a processing-continues contract |
| robots.txt | Firecrawl (SRC) | Enforced on crawl by default and fetched through the engine waterfall. Blocked URLs are reported, and overrides are a gated entitlement |
| Org compliance profile | Firecrawl Safe Mode (SRC) | Checked twice: rejected at the API and downgraded per URL at the worker. Strips credentials and actions, and blocks interactive sessions |
| URL reputation gate | Firecrawl (SRC) | Google Web Risk or the customer's Zscaler, with no stored verdicts (ZDR boundary) |
| URL cache + freshness | Firecrawl (SRC) | GCS bodies, Postgres index and Redis hot path. Supports maxAge/minAge, cacheState and negative caching |
| Cache admission policy | Firecrawl (SRC) | Never caches ZDR requests, credentialed requests, action/profile requests or low-fidelity engine output |
| Learned engine per domain | Firecrawl engpicker (SRC) | Measures TLS-vs-Chrome similarity and stores a verdict per domain |

## Differentiators (2+ sources, not most) and notable uniques
- C-005 bot-block classification and honest reporting (Codex + Apify taxonomy). This fits Verevon's stance best.
- Unique but high-value for Verevon: Safe Mode (C-018), threat protection (C-017), cache admission (C-021), Codex browser deadlines (C-009), trusted site-instruction channel (C-008), signed agent traffic (C-007).

## Verevon's biggest gaps (priority tier)
1. **C-013 robots.txt (absent, sig 5, M).** The parser and RobotsCache exist but are not applied to scrape, crawl or batch. `respect_robots` only affects the fingerprint. Proposed fix: on by default for crawl/batch and for navigations the Model proposes, with ignoring it as an audited Control Plane entitlement.
2. **C-018 org compliance profile (partial, sig 4, L).** Make RunPolicy actually enforceable: org-pinned in Control Plane, rejected at the edge, downgraded per URL in PageRunner. This is the direct form of "Space access never widens" and "Quarry executes or rejects".
3. **C-021 cache admission / ZDR (partial, sig 4, S).** The gateway search cache keeps results for 4 h, keyed without org and without a ZDR check. That breaks ZDR propagation and can leak across tenants. Fix in the gateway first, then add one shared `should_cache()`.
4. **C-005 honest bot-block handling (partial, sig 4, S).** Add a closed BlockReason enum, hostname-only telemetry, planner rules for non-signals plus one recovery attempt, and a "site blocked automation" UI state. Remove the dead `stealth.rs`.
5. **C-009 browser command deadlines (partial, sig 4, S).** Spot-check found no global per-CDP-command timeout. Add a per-op timeout table and a retry-once rule when the execution context is destroyed.

Recommended next: proxy tiers/geo behind processor gating (C-002), deadline budgeting (C-010), wire up retries (C-011), sub-coded errors (C-012), reputation gate (C-017), URL cache with freshness (C-020), gateway rate limits on browser/search routes (C-024), signed agent traffic / unified UA (C-007).
Skip: CAPTCHA solving (C-006; conflicts with policy, use HITL takeover) and demand-driven precrawl (C-022; aggregates usage across tenants, conflicts with ZDR).

## Verevon strengths (keep)
- SSRF/egress (C-015) is best in class: resolve-once DNS pinning, redirect re-validation, a per-session pinned browser proxy, a CDP Fetch guard, QUIC disabled and redacted receipts.
- Politeness (C-014): AIMD per host plus an autoscaled pool. Firecrawl only has a coarse crawl delay.
- Proxy plumbing (C-001): processor gating and per-run affinity.
- `ChallengeSignal.requires_escalation` defaults to true. Bypass is forbidden by contract.
- Agent-specific error codes: ActionUnknown, TargetRepairRequired, CheckpointLost.
- Internal Idempotency-Key from edge to control (C-025).
- Blocklists (C-016) and search providers (C-003) are at parity.

## Caveats
- All Apify records are DOC (public docs), not observed in code. The C-004 best-in-class is DOC-only.
- Clusters C-007, C-009, C-012 and C-025 have no baseline record. Their status comes from my own read-only grep spot-checks in Quarry (SRC) and needs the later verification pass. Example: chromiumoxide request timeouts were not found, but that is not proven absent.
- The baseline narratives conflict on stealth: v-browser-app says `stealth.rs` masks headless tells, while v-quarry (code-verified) says it is never injected. I followed v-quarry.
- Quarry's user agent strings are inconsistent: the default is `Quarry/2.0 (+triodelab.com/quarry)`, HTTP/3 uses `QuarryBot/1.0 (+verevon.io/bot)`, and map uses `QuarryBot`.
- Prevalence is skewed: 28 of 51 records are Firecrawl. Many "unique" clusters reflect where the sources were analysed in depth, not whether the feature exists elsewhere.
- gapClusters = 19: 20 clusters are not shipped, minus C-025, which is partial but already ahead of the reference.
