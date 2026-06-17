# Quarry V2 Roadmap

## Phase 0 — Freeze contracts ✅ scaffold
- IDs, event envelope, REST envelope, webhook schema, artifact naming, cache policy, run policy, lease schema, normalized output envelope.
- Rust `quarry-core` + Go `pkg/quarrycontracts` both generated/derived from `docs/CONTRACTS.md`.

## Phase 1 — Finish Go control plane
- Resource parity with donor: jobs, stores, snapshots, artifacts, profiles, sources.
- `list/get/history/filter` on every resource.
- Event history write path (Postgres).
- Schedule resource (separate from Temporal).
- Queue + checkpoint schemas.

## Phase 2 — Rust runtime under current API
- Build `quarry-runtime-rs` feature-flagged.
- Security engine (preflight + discovered-URL).
- Fetch engine (static/TLS), including real browser-impersonating TLS/H2/H3 via rquest upstream (`wreq`, BoringSSL). Keep `wreq-util` behind license review, `impit` as fallback experiment, and `curl_cffi` in lab only.
- Browser execution (via remote CDP or embedded chromiumoxide).
- Transform pipeline (markdown, cleaned HTML, raw HTML, links, images, attributes, metadata, change details).
- Artifact writers (s3/fs backends).
- Fingerprint/diff engine.
- Typed `DriverPlan` inspired by Firecrawl's engine waterfall: cache/index, static impersonated HTTP, browser CDP, stealth browser, document/PDF, specialty handlers.

## Phase 3 — Fast-path cutover
- `/v1/scrape` immediate path owned by `quarry-edge-rs`.
- SSE streaming owned by edge.
- Cache-hit path owned by edge (no Go hop).
- Firecrawl-compatible response adapter for common format names while retaining Quarry envelopes internally.

## Phase 4 — Browser lease model
- Durable profiles in Go (`prof_*` + s3-backed snapshots).
- Runtime leases in Rust (`lease_*`, session affinity key, sticky proxy).
- Capture/restore via Browserless-style persistent sessions and Playwright-style storage state: cookies, localStorage, sessionStorage, IndexedDB, UA, viewport, locale, timezone, reconnect/stop URLs stored as secrets.

## Phase 5 — Crawl workers to Rust
- Go Temporal: start/pause/resume/backfill/cancel.
- Rust: page execution + per-page queue + per-page progress events.
- Port Quarry V1/Firecrawl crawl behavior: sitemap, robots, include/exclude paths, depth/discovery limits, external/subdomain/backward crawling controls, typed denial reasons.
- Add Crawlee/Scrapy-inspired session health and adaptive per-host throttling.
- Temporal stays outer envelope; Rust is hot path.

## Phase 6 — Output normalization + change tracking in Rust
- Normalized output envelope (see CONTRACTS §9).
- Fingerprint (blake3), chunking, semantic-ish diff.
- Change events via envelope.
- Firecrawl format parity: markdown, html, rawHtml, links, images, screenshot, pdf, json, attributes, summary, query, branding, audio-gated, changeTracking.
- Action result artifacts: screenshots, scrape checkpoints, JS returns, PDFs.

## Phase 7 — Presets + determinism
- Go owns policy registry (presets → RunPolicy).
- Rust enforces at runtime.
- Presets must drive `DriverPlan` and crawl policy, not just request metadata.

## Phase 8 — Benchmark + retire old path
- Scoreboards: warm/cold scrape, JS success, block-rate, profile restore, schedule reliability, change precision, recovery time.
- Compare Quarry V2 against Quarry V1, local/self-hosted Firecrawl, Firecrawl cloud when available, and extraction baselines (Firecrawl Go converter, Mozilla Readability, Trafilatura).
- Cut over. Delete Go execution path in donor.

## Phase 9 — Product parity hardening
- Close remaining Firecrawl-compatible surface gaps after benchmark truth:
  - SDK ergonomics;
  - crawl errors/status/webhook parity;
  - browser interact/session APIs;
  - format-specific edge response adapters;
  - docs and migration guides.
- Keep optional high-cost features (`branding`, `audio`, LLM `summary/query/json`) feature-gated and artifact-backed.

## Future — Sovereign scraping reliability
- Build the self-owned reliability layer described in
  [`SOVEREIGN_SCRAPING_RELIABILITY.md`](SOVEREIGN_SCRAPING_RELIABILITY.md):
  domain intelligence, EgressBroker v2, session health, owned browser fleet,
  challenge classification, and benchmark gates.
- Keep page execution, transforms, artifacts, retention, and audit receipts
  inside CoreSystem; network-only proxy providers remain optional and
  policy-gated.

## Non-goals

- Retrieval/search/embeddings ⇒ Data Plane, not Quarry.
- GraphQL as source of truth ⇒ REST canonical, GraphQL optional overlay in control.
- Qdrant inside Quarry ⇒ out of main design.
- Copying Firecrawl's Node/Redis-centered control architecture.
- Embedding Crawlee/Scrapy/Playwright as wholesale frameworks in the Rust hot path.
