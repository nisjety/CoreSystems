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

## Access rule (who may call what)

**quarry-edge is the only public / cross-plane entrypoint. Never call quarry-control directly.**

| Service | Port | Auth | Who calls it | Owns |
|---|---|---|---|---|
| `quarry-edge-rs` | 8082 | **JWT** (per-tenant audience token; org from `claims.org_id`) + SSRF guards | external clients, the Verevon gateway, other planes | execution (`/v1/scrape\|crawl\|extract\|batch\|map\|search\|answer`), browser profiles (`/v1/profiles…`), SSE; **forwards** registry reads/writes to control |
| `quarry-orchestrator-go` | — | internal | edge (durable path) | Temporal workflows, schedule firing, pause/resume/backfill, webhook delivery, retries |
| `quarry-control-go` | 8081 | **HMAC** (`X-Quarry-Sig*`, `QUARRY_INTERNAL_SECRET`; signer = `crates/quarry-edge/src/internal_auth.rs`) | **edge / orchestrator / runtime only**, on a private network | durable system-of-record (jobs, schedules, sources, snapshots, artifacts, profiles, event log, webhooks) in Postgres + the dispatcher/cron |

Why this matters for callers:

- **Control has no JWT/org awareness.** It trusts HMAC-signed internal traffic and degrades to "trust the network" when the secret is unset (rollout mode). A caller that bypasses edge gets *no per-tenant scoping* — and once `QUARRY_INTERNAL_HMAC_REQUIRED=1`, an unsigned caller is rejected with 401.
- **Control does not expose execution or live-profile endpoints.** `/v1/scrape|crawl|extract|batch` and `/v1/profiles/{id}/restore_probe` exist on **edge only**; control holds the durable `/v1/jobs` registry and a different `/v1/restore` shape. Wiring a dashboard/registry consumer to control 404s those calls.
- **Edge's registry surface is a forwarding proxy.** Edge's `/v1/jobs`, `/v1/schedules`, `/v1/sources`, `/v1/runs/{id}/events` forward to `control_base_url` (default `http://quarry-control:8081`) over HMAC; when `control_base_url` is unset they return empty pages (dev/test). So edge is the contract surface even for "just reading the registry."

Practical consequence: the Verevon v3 gateway's `ingestions`, `knowledge`, and `search` domains all target `QUARRY_EDGE_URL` with a minted `quarry` audience token. `quarry-control` is never a gateway/cross-plane upstream — see also `CROSS_PLANE_AUTH.md`.

The one current exception is verevonv3's **onboarding** crawl handlers, which post to control `/v1/jobs/` directly. That path works only because control runs in HMAC **rollout mode** (unsigned requests trusted on the private network); it carries no org scoping and is rejected once `QUARRY_INTERNAL_HMAC_REQUIRED=1`. It is a known wart to migrate onto edge — not a pattern to extend.

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
