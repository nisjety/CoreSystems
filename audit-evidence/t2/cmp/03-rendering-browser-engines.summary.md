# T2 cat 03: Rendering and browser engines (cross-source comparison)

Inputs: 59 reference records (apify 10, aside 5, codexbrowser 13, firecrawl 31) and 24 Verevon baseline records. Result: 31 clusters, 58 records assigned, 1 excluded (firecrawl-engine-and-infra-005, a note about Firecrawl's open-core split rather than a capability). Detail is in `03-rendering-browser-engines.clusters.json`.

Two kinds of source are compared here. Apify and Firecrawl are web-crawl benchmarks. Aside and Codex are user-facing agent browsers. "Table-stakes" is therefore judged within the relevant group.

## Shared baseline (table-stakes)

| Capability | Verevon |
|---|---|
| Multi-tier engine core: HTTP, TLS-impersonating HTTP and browser (C001) | **shipped**: static, TLS (wreq) and chromiumoxide drivers sit behind FallbackDriver [SRC] |
| TLS/H2 impersonation plus consistent headers (C005) | partial: Chrome profile only, the TLS driver does not follow redirects [SRC] |
| Browser pool lifecycle and context isolation (C007) | partial: one shared Chromium process. A lease with a different viewport relaunches the browser and clears every session's pages [INF from `ensure_browser`]. The lease pool size is 2 [SRC] |
| Persistent sessions and profiles (C008) | partial: CRUD, scopes and the restore probe ship. Snapshots lose cookie expiry and IndexedDB and are not encrypted at rest [SRC] |
| Page actions on scrape (C010) | partial: agent and UI actions ship. `/v1/scrape` only has `waitForSelector`, and ActionRuntime is unrouted [SRC] |

## Best of breed

| Capability | Best source (records) | Why |
|---|---|---|
| Engine core | Apify Crawlee (apify-054) | One crawler hierarchy covering HTTP and Playwright/Puppeteer across 3 browser engines [DOC] |
| Engine selection | Firecrawl (fc-eng-001) | Weighted matrix of 17 feature flags per engine; callers never name an engine [SRC] |
| Learned rendering mode | Apify (apify-055) | HTTP-first, with 10% of requests run in both modes and compared; the mode is learned per site [DOC] |
| Hedged waterfall and re-plan | Firecrawl (fc-eng-002/003) | Races engines past a deadline; typed AddFeature/RemoveFeature errors with retry budgets [SRC] |
| Fingerprints and stealth | Apify (apify-056/057) | Statistical fingerprints tied to the proxy session; Camoufox [DOC] |
| Pool lifecycle | Apify BrowserPool (apify-058) | maxOpenPages, retire-after-N, incognito pages, ordered hooks [DOC] |
| Profiles | Firecrawl (fc-api-058, fc-eng-106) | Profiles not allowed under ZDR; monotonic `saved_at` and deletion tombstones [SRC] |
| Profile routing | Firecrawl (fc-eng-018) | Requests with a profile are pinned to the browser and never use the shared cache [SRC] |
| Scrape actions | Firecrawl (fc-api-057, fc-eng-012) | 9 action types, 50 actions / 60 s cap, all lowered into one atomic list [SRC] |
| Geo, locale, mobile | Firecrawl (fc-api-060) | ISO country plus special pools, languages; options partition the cache [SRC] |
| Background tab hygiene | Aside (aside-029/085) | Active lifecycle, focus emulation and a stable 1440x900 viewport [SRC] |
| Settle after actions | Codex (aas-058) | Commit, then first paint, then 50–100 ms of AX quiet with a 750 ms cap [SRC] |
| Frame-aware AX snapshot | Codex (aas-057) | Merges OOPIFs; per-frame warnings and deadlines [SRC] |
| Fresh screenshots | Codex (aas-061) | Takes a screencast frame newer than the request [SRC] |
| Multi-backend browser | Codex (iab-009, aas-139) | Embedded, user Chrome via extension, CDP cloud and MCP apps, each with its own capabilities [SRC] |
| File routing | Firecrawl (fc-eng-017) | Content-Type then magic bytes, passing the already-fetched bytes to the parser [SRC] |
| Hosted session lifecycle | Firecrawl Hangar (fc-eng-019) | State machine, idle and absolute TTLs, idempotent create, view and control live-view URLs [SRC] |
| Browser SSRF | **Verevon** (beats fc-eng-009) | Per-session loopback proxy that resolves DNS once, a CDP Fetch guard and QUIC disabled [SRC] |

## Differentiators (2 or more sources, not most)

- Background tab hygiene (Aside, Codex).
- Post-action settle (Codex, plus Firecrawl at INF, low confidence).
- Frame-aware AX snapshot (Codex, Firecrawl).
- Hosted session lifecycle and live view (Firecrawl, Apify).
- Fingerprint/stealth tier (Apify, Firecrawl). Codex deliberately has none because it drives the user's real browser.

## Biggest gaps for Verevon (priority tier)

1. **C007 Browser pool.** Apply the viewport per page with `setDeviceMetricsOverride` instead of relaunching the browser. Add a pool of K processes with maxOpenPages, retire-after-N and a real health probe. Effort M, Ingestion Plane.
2. **C008 Profiles.** Capture cookie expiry, storage for every origin and IndexedDB. Envelope-encrypt snapshots with org keys obtained through the Control Plane API. Add monotonic saves and tombstones, and wire the S3 store. Keep profiles off for ZDR orgs. Effort M.
3. **C010 Scrape actions.** Route the existing ActionRuntime through `ScrapeRequest.actions` with Firecrawl's caps. `evaluate` is only allowed with an approval grant. The Model proposes and Quarry validates and executes. Effort M.
4. **C024 File routing.** Add a content sniffer to the scrape pipeline and send files to the existing lopdf/docx/image parsers. Today a PDF goes through readability. Effort S, and users will notice the difference.

Recommended next:
- Capability-matrix planner (C002), typed re-plan and budgets (C004) and learned per-domain mode (C003).
- Locale, timezone and geolocation overrides plus country egress (C011). Stealth wired under a policy-gated preset (C006).
- Settle after agent actions (C014). AX snapshot warnings and deadlines (C015).
- Screenshot on scrape (C016). Remote-provider containment proof so Kernel, Browserless and Browserbase can run agent sessions (C017).
- Durable sessions with idle and absolute TTLs and suspend/resume (C021). Bounds and approval on today's unrestricted Quarry `evaluate` (C022).
- URL rewrites plus a provider resolver that calls integration-corev2 over its API (C025).

Skip, because they conflict with the architecture or are out of scope:
- Embedded or native browser shell (C019). The browser must stay a remote leased Chromium.
- Desktop computer use (C020).
- Async fire-engine protocol (C028).
- Virtual clipboard (C030). Remote isolation already protects the user's clipboard.
- Agent-controlled privacy settings (C031).
- Driving the user's own browser through an extension (C018) is nice-to-have. It has high value, but it gives up egress containment and ZDR evidence.

## Verevon strengths (keep)

- **Browser SSRF and egress (C027).** Ahead of every reference: one DNS resolution per session pinned to the proxy, a CDP Fetch guard, QUIC disabled, per-hop redirect checks and egress receipts.
- **Engine core (C001).** Static, TLS and browser drivers with JS-shell escalation and soft-404/ChallengeKind classification.
- **Debug collectors (C029).** CDP-level console, log, request, response and lifecycle collectors plus a DevTools drawer. Pages cannot tamper with them, unlike Aside's in-page interceptor.
- **Isolation.** A per-session BrowserContext, download dir and proxy. Capability flags (`isolated_egress`, `security_evidence`) refuse unproven backends.
- **Live view.** HTTP/SSE frames, takeover and a timeline, with frames marked `no-store`.

## Caveats

- Apify records are all DOC (vendor docs). Firecrawl's hosted behaviour (fire-engine, Hangar) is seen only through the client contract in the OSS repo. The smart-wait cache (fc-eng-109) is INF with low confidence.
- These Verevon statuses rest on greps, not the baseline records, so verify them in code:
  - no focus emulation or lifecycle forcing (C013)
  - no agent post-action settle (C014)
  - no per-frame AX warnings (C015)
  - no ad blocking (C012)
- The ref record that says "Quarry blocks ads via CDP Fetch lists" conflicts with the baseline. I treat the CDP Fetch guard as an SSRF guard only.
- The viewport-relaunch problem (C007) is an inference from `ensure_browser` (`chromiumoxide.rs:194-226`). Confirm it with a concurrent-session test.
- `profile_required` in DriverPlan is never set by any caller (grep), so scrape cannot use profiles today (C009).
- The rendered-branding status (C026) comes from `pipeline.rs:562`: it extracts from the HTML string, not from computed styles.
- Sessions in the UI are capped at 12 steps and 120 s, and the AI loop at 5 steps. Several gaps (settle, snapshot quality) matter more because of these budgets.
