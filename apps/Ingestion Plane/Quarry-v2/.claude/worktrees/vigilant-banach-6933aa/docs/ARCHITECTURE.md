# Architecture

## Plane split

```
                                 ┌─────────────────────────────────────────┐
       client ──REST/SSE────────▶│ quarry-edge-rs      (public ingest)     │
                                 │  auth, validate, preflight, cache,      │
                                 │  SSE, fast-path dispatch                │
                                 └─────────────┬───────────┬───────────────┘
                                               │           │
                            fast path          │           │  durable path
                                               ▼           ▼
                          ┌──────────────────────────┐    ┌─────────────────────────────┐
                          │ quarry-runtime-rs        │    │ quarry-orchestrator-go       │
                          │  driver select, fetch,   │    │  Temporal WFs, schedules,    │
                          │  browser leases, actions,│    │  pause/resume/backfill,      │
                          │  transform, artifacts,   │◀───│  webhook delivery, retries   │
                          │  fingerprint/diff, events│    └──────────────┬───────────────┘
                          └─────────────┬────────────┘                   │
                                        │ events                         │ records
                                        ▼                                ▼
                                 ┌─────────────────────────────────────────┐
                                 │ quarry-control-go   (resources + history)│
                                 │  jobs, stores, snapshots, artifacts,     │
                                 │  profiles, schedules, event log, webhook │
                                 └─────────────────────────────────────────┘
                                                │
                                      Postgres ─┴─ Redis ─── S3/FS
```

## Why this split

- **Hot path in Rust** — concurrency, latency, memory, stateful drivers and browser sessions. Donor's `internal/driver/`, `internal/scraper/`, `internal/transform/`, `internal/pipeline/`, and page-execution half of `internal/crawl/runner.go` are the Rust territory.
- **Durable control in Go** — CRUD parity, list/history/filter, Temporal orchestration, webhooks, policy registry. Donor's `internal/temporal/`, resource endpoints, profile metadata, schedule APIs stay Go.
- **Lab in Python** — experiments only. Outputs are static (prompts, schemas, classifiers) consumed by Rust runtime via config or artifacts.

## Donor and competitor strategy

Quarry V2 is not a blind rewrite. It has three explicit reference systems:

| System | Use as | What to copy | What not to copy |
|--------|--------|--------------|------------------|
| Quarry V1 (`../Quarry`) | Feature donor | API parity, crawl semantics, prompt/schema extraction behavior, session/profile UX, security heuristics, search/map/extract/interact lessons | Go-everywhere hot path, middleware-as-policy, tangled scraper/driver/control boundaries |
| Firecrawl (`/Volumes/Lagring/Triodelab/firecrawl`) | Product and engine donor | Format surface, action schema, engine waterfall, crawl denial reasons, transform chain, SDK ergonomics, cache/index-first path | Large Node control surface, Redis TTL as sole durable crawl truth, dependency on non-self-hosted Fire Engine for advanced anti-bot |
| Browserless/Crawlee/Apify/Playwright ecosystem | Runtime pattern donor | Persistent sessions, browser context snapshots, session pool health scoring, adaptive crawl concurrency, proxy/session affinity | Full framework takeover inside Quarry runtime |

Current posture:

- Quarry V1 is broader than V2 today, but V2 has the better execution/control split.
- Firecrawl is broader than V2 today, but V2 can be stronger for self-hosted durability, observability, recoverability, profile ownership, and deterministic outputs.
- V2 should borrow product semantics from V1/Firecrawl while keeping Rust as the page execution boundary and Go as the durable control boundary.

## Engine waterfall

Firecrawl's local code has a mature engine waterfall: index/cache, Fire Engine CDP, Fire Engine TLS client, Playwright, fetch, PDF/document, and Wikipedia specialty paths. Quarry V2 should implement the same idea as a typed Rust `DriverPlan`, not as ad hoc fallback.

```
DriverPlan
  ├─ index/cache hit            (fastest; no network when valid)
  ├─ static impersonated HTTP   (rquest upstream / wreq crate + BoringSSL TLS/H2 emulation)
  ├─ browser CDP                (chromiumoxide/local Chrome or Browserless CDP)
  ├─ browser CDP + stealth      (sticky proxy + persisted profile + action support)
  ├─ document/PDF parser        (PDF/DOCX/XLSX/RTF path, never HTML assumptions)
  └─ specialty handlers         (Wikipedia, YouTube/audio, future site adapters)
```

Selection inputs:

- Requested formats and actions.
- Policy preset (`fast`, `polite`, `stealth`, `deterministic`, future custom).
- URL hints: file extension, known JS-required hosts, known static/API hosts.
- Cache policy and age.
- Session/profile requirement.
- Block signals from prior attempts.
- Budget: timeout, cost tier, max browser time.

Selection outputs:

- Chosen driver and ordered fallback list.
- Unsupported feature set per fallback.
- Driver reason codes for events and `meta.json`.
- Retry/escalation plan: retry static, escalate to TLS, escalate to browser, or abort.

The first implementation should keep this deterministic and inspectable: every fallback decision must be visible in events and artifact metadata.

## Product format pipeline

Firecrawl's strongest product advantage is not one scraper. It is its format pipeline. Quarry V2 should treat formats as first-class transform requests, backed by artifacts:

| Format | Runtime owner | Notes |
|--------|---------------|-------|
| `markdown` | Rust transform | Readability-first; benchmark against Firecrawl Go converter, Mozilla Readability, Trafilatura |
| `html` | Rust transform | Cleaned HTML after tag/include/exclude/main-content processing |
| `raw_html` | Runtime driver | Unmodified response/browser content |
| `links` | Rust transform | Absolute URLs with text/rel/source selector where possible |
| `images` | Rust transform | Absolute image URLs + alt/title/dimensions when available |
| `screenshot` | Browser driver | Action-derived or format-derived artifact |
| `pdf` | Browser/document path | Browser print-to-PDF or direct PDF parser |
| `attributes` | Rust transform | CSS selector + attribute extraction |
| `json` | Control/lab assisted | Schema/prompt extraction job; runtime stores artifact result |
| `summary` | Control/lab assisted | LLM transform, not hot-path dependency |
| `query` | Control/lab assisted | Page-level answer over markdown/html |
| `branding` | Browser + transform | Use Firecrawl branding script as behavior reference; output colors/fonts/logo/components |
| `audio` | Specialty handler | YouTube/media extraction belongs behind optional feature gate |
| `change_tracking` | Rust transform | Fingerprints + paragraph diff + optional JSON/git-diff style artifacts |

Runtime should always produce artifacts and metadata even if the edge response filters fields for compatibility.

## Not in scope

- Retrieval / embeddings / search → Data Plane, not Quarry.
- GraphQL as source of truth → REST canonical, GraphQL optional overlay.
- Qdrant inside Quarry → out of main design.

## Middleware ≠ policy

Donor Fiber middleware mixed transport concerns (auth, rate limit) with runtime behaviors (timing, proxy rotation, retry). V2 separation:

| Concern               | Where                                       |
|-----------------------|---------------------------------------------|
| auth, rate limit, body limits, request-id | edge transport middleware (axum towers)     |
| timing / jitter       | RunPolicy.delay → enforced in runtime       |
| proxy rotation        | RunPolicy.proxy + browser lease affinity    |
| retry / block escalation | RunPolicy.retry + .block → runtime retry.rs  |
| cache policy          | CachePolicy → edge resolution + runtime emit |

## Contracts are source of truth

Every type shipped across the plane boundary is defined in `docs/CONTRACTS.md`. Rust side: `crates/quarry-core`. Go side: `pkg/quarrycontracts`. Schema-level parity enforced by contract tests (Phase 0 Todo).

## Browser lease model

No "browser pool" abstraction. Runtime exposes:

- **lease** (`lease_*`) — runtime-side handle, TTL + capabilities + artifact bucket.
- **profile** (`prof_*`) — control-side durable resource, snapshotable.
- **session affinity key** — sticky reconnect, preserves cookies/storage.
- **proxy affinity** — sticky routing pair with session.

Browserless reconnects, ScrapingBee `session_id`, and durable profiles all collapse onto this shape.

Implementation rule after the Firecrawl/Browserless comparison: one-shot `/content`-style Browserless calls are useful fallback only. The production lease path must support persistent CDP/BQL-style sessions, stored reconnect/session URLs, explicit cleanup, and profile snapshots with cookies, localStorage, sessionStorage, IndexedDB, user agent, viewport, proxy affinity, and validation probes.

## Crawl frontier model

Quarry V1 and Firecrawl both have useful crawl behavior, but neither is the final V2 shape.

V2 frontier components:

- `RequestQueue`: per-run queue with priority, depth, source URL, canonical URL, retry count, and reason fields.
- `SeenSet`: canonical URL dedupe with configurable query stripping and section-anchor policy.
- `HostSlot`: per-host delay/concurrency state, robots crawl-delay floor, and adaptive backoff.
- `SessionPool`: session/proxy health scores (`good`, `bad`, `retired`) inspired by Crawlee.
- `DenialReason`: structured reason for skipped links, using Firecrawl-style operator-readable explanations.
- `Checkpoint`: serializable frontier + seen-set + host-slot state for Temporal replay.

Temporal remains the durable outer workflow. Rust owns page execution, frontier mutation, link extraction, denial classification, and per-page events.

## Dependency posture

Use stronger dependencies when they create measurable advantage:

- Rust hot path: use the `rquest` upstream transport, published as the `wreq` crate on crates.io, for BoringSSL-backed TLS/JA3/JA4/HTTP/2 browser emulation. Keep `wreq-util` profile catalogs deferred until license review, and keep `impit` as a documented fallback experiment only if the target corpus proves `wreq` insufficient.
- Go control plane: keep `chi`, `pgx`, Temporal SDK, and small explicit stores; avoid pulling scraper frameworks into control.
- Python lab: use `curl_cffi`, Crawlee Python, Trafilatura, Playwright, and eval tooling as benchmark/lab dependencies only.
- Browser: prefer CDP-compatible drivers and Browserless-style persistent sessions over bespoke browser automation.

Any dependency entering the hot path needs a small adapter trait, license check, security review, reproducible benchmark, and fallback path.
