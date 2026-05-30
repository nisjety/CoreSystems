# Quarry V2 — Goal

## North star

Ship a **two-plane ingestion engine** that captures, normalizes, and durably tracks web content at production scale. Rust runs hot, Go runs durable, Python stays in the lab.

Beats the donor (`../Quarry`, Go-everywhere) on: latency, JS-heavy success rate, session continuity, change-detection precision, operator recovery, and hot-path maintainability. Beats self-hosted Firecrawl on: durability, self-owned anti-bot/runtime choices, profile/session persistence, event history, crash recovery, inspectable driver decisions, and deterministic artifacts. Matches or exceeds Firecrawl on API ergonomics, format breadth, actions, crawl diagnostics, and output contract clarity. Matches Apify/Crawlee on durable resources and session/proxy health. Matches Browserless on session reuse, reconnect, and sticky proxy.

## Competitive thesis

### Quarry V1

Quarry V1 is the feature donor, not the final architecture. It is broader today: AI extraction, schema/prompt behavior, Rod sessions, sitemap/robots crawl logic, search/map/extract/interact endpoints, and production-shaped API docs. V2 must port the valuable behavior while avoiding the V1 failure mode: too many concerns in one Go runtime and too much scrape behavior hidden behind middleware.

V2 wins by moving execution into Rust and durability into Go, while using V1 as an API and behavior parity checklist.

### Firecrawl

Firecrawl is the product benchmark. Its local repo shows a mature TypeScript engine waterfall, broad v2 format schema, transform pipeline, SDKs, Playwright microservice, Go markdown service, Redis/RabbitMQ queueing, and strong public API ergonomics. It is ahead today on user-facing breadth.

Firecrawl's self-hosted weakness is that the advanced Fire Engine paths are not generally included, so the best CDP/TLS anti-bot behavior is partly a managed-service advantage. Quarry V2 should close the feature gap and then win in self-hosted environments by making the runtime inspectable, durable, and replaceable.

### External systems

- Browserless: persistent sessions, reconnect URLs, explicit stop URLs, BrowserQL/CDP duality.
- Playwright: browser contexts, storage state, IndexedDB capture, reliable action model.
- Crawlee: request queue, session pool, proxy affinity, health scoring, autoscaled concurrency.
- Scrapy: per-host AutoThrottle, especially the rule that fast error responses must not make the crawler more aggressive.
- Mozilla Readability and Trafilatura: extraction quality baselines for main-content and metadata.
- rquest upstream (`wreq`), with BoringSSL, is the default TLS/JA3/JA4/H2 impersonation path. `wreq-util` remains behind license review; `impit` is a fallback experiment; `curl_cffi` is a Python lab baseline only.

## Non-negotiables

1. **Rust owns execution.** Driver select, fetch, browser, actions, transform, fingerprint/diff, artifact write, retry/block. No Go in the per-page hot path.
2. **Go owns durability.** Jobs, stores, snapshots, artifacts, profiles, schedules, event history, Temporal orchestration, webhook delivery. No Rust in the control plane.
3. **Python is lab-only.** Prompts, evasion experiments, classifiers, eval harnesses. Outputs feed runtime via static config or artifacts — never via live RPC.
4. **REST is canonical.** GraphQL overlay optional. No alternate source of truth.
5. **Contracts frozen at v1.** IDs, envelopes, event types, policy schemas are additive-only. Breaking change ⇒ `/v2/*` namespace.
6. **No retrieval plane.** Embeddings, search, ranking live in Data Plane. Quarry publishes clean outputs; nothing more.
7. **Middleware ≠ policy.** Timing, proxy, retry are `RunPolicy` attributes enforced by runtime. Not generic HTTP middleware.
8. **Browser lease model.** No pool abstraction. Leases + profiles + session-affinity + sticky-proxy. Period.
9. **Engine decisions are observable.** Every cache hit, driver choice, fallback, retry, escalation, and unsupported feature must be emitted or written to metadata.
10. **Feature breadth must not pollute the hot path.** LLM extraction, summary, query, branding analysis, and audio are optional transforms/jobs over captured artifacts; they cannot become mandatory live dependencies for basic scrape.

## Success criteria

Measured in Phase 8 scoreboard. All vs. donor Quarry and vs. Firecrawl:

| Metric                      | Target (v1.0)                       |
|-----------------------------|-------------------------------------|
| Warm scrape p50             | ≤ 120ms                             |
| Cold scrape p50             | ≤ 1500ms                            |
| Cold scrape p95             | ≤ 4000ms                            |
| JS-heavy success rate       | ≥ 92% on stealth benchmark suite    |
| Block-rate on clean traffic | ≤ 0.5% false positives              |
| Profile restore success     | ≥ 99% within 30 days of capture     |
| Schedule firing reliability | ≥ 99.9% within 5s of cron trigger   |
| Change-detection precision  | ≥ 95% on known-changed gold set     |
| Operator recovery after crash | ≤ 10s (Temporal replay)            |
| Code reuse from donor       | ≥ 60% of logic ported (not rewritten) |
| Firecrawl format parity     | 100% for markdown/html/raw/links/images/screenshot/pdf/json/attributes/changeTracking; branding/audio may be gated |
| Crawl denial explainability | 100% skipped URLs carry typed reason + operator message |
| Driver decision observability | 100% scrapes write driver plan metadata |

## Anti-goals

- Rewriting the REST boundary. Client contract stays.
- Rewriting Temporal workflow shape. Outer envelope is proven.
- Building our own browser. Use local Chromium or remote Browserless-style infra via CDP.
- Building a vector DB inside Quarry. Qdrant is out.
- "Framework unification" refactors that block feature delivery.
- Copying Firecrawl's Node control architecture. Firecrawl is a product donor, not a plane-boundary donor.
- Putting Crawlee/Scrapy/Playwright as frameworks inside the hot path. Borrow the patterns; keep the adapters narrow.

## Guiding tensions

- **Speed vs. safety.** Preflight + SSRF block > scrape-first. Security wins ties.
- **Determinism vs. yield.** `RunPolicy.determinism` is a user-facing knob, not a default.
- **Compatibility vs. clarity.** Donor has cruft; don't port the cruft. Port the logic.
- **Control vs. runtime.** When in doubt, put state in Go, behavior in Rust.
- **Breadth vs. blast radius.** Add Firecrawl-compatible features as independent transforms/artifacts, not as one giant scrape function.
- **Managed anti-bot vs. self-hosted ownership.** Use Browserless/proxy providers when configured, but keep first-party driver adapters and fallback paths.

## Definition of done (v1.0)

- Phase 0–6 complete (see `ROADMAP.md`).
- Scoreboard targets above met or documented.
- Donor Go execution path removed or frozen.
- Client-facing REST + SSE unchanged from donor (modulo additive v1 fields).
- Ops runbook, migration guide, and failure-mode catalog published.
- Firecrawl/V1 parity matrix published with unsupported items explicitly marked and justified.
- `docs/SCOREBOARD.md` includes Quarry V1, local/self-hosted Firecrawl, and Quarry V2 results on the same corpus.
